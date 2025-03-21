use std::collections::HashMap as Map;

use async_trait::async_trait;
use chrono::prelude::*;
pub mod proof;
pub use proof::{Check, LinkedDataProofOptions, Proof};
pub mod error;
pub use error::Error;
pub mod context;
pub mod soltx;
pub use context::Context;

#[cfg(feature = "eip")]
pub mod eip712;

// use crate::did::{VerificationMethod, VerificationMethodMap};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ssi_core::uri::URI;
use ssi_crypto::hashes::sha256::sha256;
use ssi_dids::did_resolve::{resolve_key, DIDResolver};
use ssi_dids::VerificationRelationship as ProofPurpose;
use ssi_json_ld::{rdf::DataSet, urdna2015, ContextLoader};
use ssi_jwk::{Algorithm, Base64urlUInt, JWK};
use ssi_jws::Header;

pub mod suites;
pub use suites::*;

// TODO: factor out proof types
lazy_static::lazy_static! {
    /// JSON-LD context for Linked Data Proofs based on Tezos addresses
    pub static ref TZ_CONTEXT: Value = {
        let context_str = ssi_contexts::TZ_V2;
        serde_json::from_str(context_str).unwrap()
    };
    pub static ref TZVM_CONTEXT: Value = {
        let context_str = ssi_contexts::TZVM_V1;
        serde_json::from_str(context_str).unwrap()
    };
    pub static ref TZJCSVM_CONTEXT: Value = {
        let context_str = ssi_contexts::TZJCSVM_V1;
        serde_json::from_str(context_str).unwrap()
    };
    pub static ref EIP712VM_CONTEXT: Value = {
        let context_str = ssi_contexts::EIP712VM;
        serde_json::from_str(context_str).unwrap()
    };
    pub static ref EPSIG_CONTEXT: Value = {
        let context_str = ssi_contexts::EPSIG_V0_1;
        serde_json::from_str(context_str).unwrap()
    };
    pub static ref SOLVM_CONTEXT: Value = {
        let context_str = ssi_contexts::SOLVM;
        serde_json::from_str(context_str).unwrap()
    };
    pub static ref ALEOVM_CONTEXT: Value = {
        let context_str = ssi_contexts::ALEOVM;
        serde_json::from_str(context_str).unwrap()
    };
}
// https://w3c-ccg.github.io/vc-http-api/#/Verifier/verifyCredential
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
/// Object summarizing a verification
/// Reference: vc-http-api
pub struct VerificationResult {
    /// The checks performed
    pub checks: Vec<Check>,
    /// Warnings
    pub warnings: Vec<String>,
    /// Errors
    pub errors: Vec<String>,
}

impl VerificationResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error(err: &str) -> Self {
        Self {
            checks: vec![],
            warnings: vec![],
            errors: vec![err.to_string()],
        }
    }

    pub fn append(&mut self, other: &mut Self) {
        self.checks.append(&mut other.checks);
        self.warnings.append(&mut other.warnings);
        self.errors.append(&mut other.errors);
    }

    pub fn with_error(mut self, error: String) -> Self {
        self.errors.push(error);
        self
    }
}

impl From<Result<VerificationWarnings, Error>> for VerificationResult {
    fn from(res: Result<VerificationWarnings, Error>) -> Self {
        match res {
            Ok(warnings) => Self {
                checks: vec![],
                warnings,
                errors: vec![],
            },
            Err(error) => Self {
                checks: vec![],
                warnings: vec![],
                errors: vec![error.to_string()],
            },
        }
    }
}

macro_rules! feature_gate {
    ($name:literal, $type:ident) => {{
        #[cfg(not(feature = $name))]
        return Err(Error::JWS(ssi_jws::Error::MissingFeatures($name)));
        #[cfg(feature = $name)]
        &$type
    }};
}

pub fn get_proof_suite(proof_type: &str) -> Result<&(dyn ProofSuite + Sync), Error> {
    Ok(match proof_type {
        "RsaSignature2018" => feature_gate!("w3c", RsaSignature2018),
        "Ed25519Signature2018" => feature_gate!("w3c", Ed25519Signature2018),
        "Ed25519Signature2020" => feature_gate!("w3c", Ed25519Signature2020),
        "Ed25519BLAKE2BDigestSize20Base58CheckEncodedSignature2021" => {
            feature_gate!(
                "tezos",
                Ed25519BLAKE2BDigestSize20Base58CheckEncodedSignature2021
            )
        }
        "P256BLAKE2BDigestSize20Base58CheckEncodedSignature2021" => {
            feature_gate!(
                "tezos",
                P256BLAKE2BDigestSize20Base58CheckEncodedSignature2021
            )
        }
        "EcdsaSecp256k1Signature2019" => feature_gate!("w3c", EcdsaSecp256k1Signature2019),
        "EcdsaSecp256k1RecoverySignature2020" => {
            feature_gate!("w3c", EcdsaSecp256k1RecoverySignature2020)
        }
        "Eip712Signature2021" => {
            feature_gate!("eip", Eip712Signature2021)
        }
        "EthereumPersonalSignature2021" => {
            feature_gate!("eip", EthereumPersonalSignature2021)
        }
        "EthereumEip712Signature2021" => {
            feature_gate!("eip", EthereumEip712Signature2021)
        }
        "TezosSignature2021" => feature_gate!("tezos", TezosSignature2021),
        "TezosJcsSignature2021" => feature_gate!("tezos", TezosJcsSignature2021),
        "SolanaSignature2021" => feature_gate!("solana", SolanaSignature2021),
        "AleoSignature2021" => {
            feature_gate!("aleo", AleoSignature2021)
        }
        "JsonWebSignature2020" => feature_gate!("w3c", JsonWebSignature2020),
        "EcdsaSecp256r1Signature2019" => feature_gate!("w3c", EcdsaSecp256r1Signature2019),
        _ => return Err(Error::ProofTypeNotImplemented),
    })
}

fn pick_proof_suite<'a, 'b>(
    jwk: &JWK,
    verification_method: Option<&'a URI>,
) -> Result<&'b (dyn ProofSuite + Sync), Error> {
    let algorithm = jwk.get_algorithm().ok_or(Error::MissingAlgorithm)?;
    Ok(match algorithm {
        Algorithm::RS256 => feature_gate!("w3c", RsaSignature2018),
        Algorithm::PS256 => feature_gate!("w3c", JsonWebSignature2020),
        Algorithm::ES384 => feature_gate!("w3c", JsonWebSignature2020),
        Algorithm::AleoTestnet1Signature => feature_gate!("aleo", AleoSignature2021),
        Algorithm::EdDSA | Algorithm::EdBlake2b => match verification_method {
            Some(URI::String(ref vm))
                if (vm.starts_with("did:sol:") || vm.starts_with("did:pkh:sol:"))
                    && vm.ends_with("#SolanaMethod2021") =>
            {
                feature_gate!("solana", SolanaSignature2021)
            }
            Some(URI::String(ref vm))
                if vm.starts_with("did:tz:") || vm.starts_with("did:pkh:tz:") =>
            {
                if vm.ends_with("#TezosMethod2021") {
                    feature_gate!("tezos", TezosSignature2021)
                } else {
                    feature_gate!(
                        "tezos",
                        Ed25519BLAKE2BDigestSize20Base58CheckEncodedSignature2021
                    )
                }
            }
            _ => feature_gate!("w3c", Ed25519Signature2018),
        },
        Algorithm::ES256 | Algorithm::ESBlake2b => match verification_method {
            Some(URI::String(ref vm))
                if vm.starts_with("did:tz:") || vm.starts_with("did:pkh:tz:") =>
            {
                if vm.ends_with("#TezosMethod2021") {
                    feature_gate!("tezos", TezosSignature2021)
                } else {
                    feature_gate!(
                        "tezos",
                        P256BLAKE2BDigestSize20Base58CheckEncodedSignature2021
                    )
                }
            }
            _ => feature_gate!("w3c", JsonWebSignature2020),
        },
        Algorithm::ES256K | Algorithm::ESBlake2bK => match verification_method {
            Some(URI::String(ref vm))
                if vm.starts_with("did:tz:") || vm.starts_with("did:pkh:tz:") =>
            {
                if vm.ends_with("#TezosMethod2021") {
                    feature_gate!("tezos", TezosSignature2021)
                } else {
                    feature_gate!("w3c", EcdsaSecp256k1RecoverySignature2020)
                }
            }
            _ => feature_gate!("w3c", EcdsaSecp256k1Signature2019),
        },
        Algorithm::ES256KR =>
        {
            #[allow(clippy::if_same_then_else)]
            if use_eip712sig(jwk) {
                feature_gate!("eip", EthereumEip712Signature2021)
            } else if use_epsig(jwk) {
                feature_gate!("eip", EthereumPersonalSignature2021)
            } else {
                match verification_method {
                    Some(URI::String(ref vm))
                        if (vm.starts_with("did:ethr:") || vm.starts_with("did:pkh:eth:"))
                            && vm.ends_with("#Eip712Method2021") =>
                    {
                        feature_gate!("eip", Eip712Signature2021)
                    }
                    _ => feature_gate!("w3c", EcdsaSecp256k1RecoverySignature2020),
                }
            }
        }
        _ => return Err(Error::ProofTypeNotImplemented),
    })
}
// Get current time to millisecond precision if possible
pub fn now_ms() -> DateTime<Utc> {
    let datetime = Utc::now();
    let ms = datetime.timestamp_subsec_millis();
    let ns = ms * 1_000_000;
    datetime.with_nanosecond(ns).unwrap_or(datetime)
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait LinkedDataDocument {
    fn get_contexts(&self) -> Result<Option<String>, Error>;
    fn to_value(&self) -> Result<Value, Error>;
    fn get_default_proof_purpose(&self) -> Option<ProofPurpose> {
        None
    }
    fn get_issuer(&self) -> Option<&str> {
        None
    }
    async fn to_dataset_for_signing(
        &self,
        parent: Option<&(dyn LinkedDataDocument + Sync)>,
        context_loader: &mut ContextLoader,
    ) -> Result<DataSet, Error>;
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ProofSuite {
    async fn sign(
        &self,
        document: &(dyn LinkedDataDocument + Sync),
        options: &LinkedDataProofOptions,
        resolver: &dyn DIDResolver,
        context_loader: &mut ContextLoader,
        key: &JWK,
        extra_proof_properties: Option<Map<String, Value>>,
    ) -> Result<Proof, Error>;

    async fn prepare(
        &self,
        document: &(dyn LinkedDataDocument + Sync),
        options: &LinkedDataProofOptions,
        resolver: &dyn DIDResolver,
        context_loader: &mut ContextLoader,
        public_key: &JWK,
        extra_proof_properties: Option<Map<String, Value>>,
    ) -> Result<ProofPreparation, Error>;

    async fn complete(
        &self,
        preparation: ProofPreparation,
        signature: &str,
    ) -> Result<Proof, Error>;

    async fn verify(
        &self,
        proof: &Proof,
        document: &(dyn LinkedDataDocument + Sync),
        resolver: &dyn DIDResolver,
        context_loader: &mut ContextLoader,
    ) -> Result<VerificationWarnings, Error>;
}

pub use ssi_jws::VerificationWarnings;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProofPreparation {
    pub proof: Proof,
    pub jws_header: Option<Header>,
    pub signing_input: SigningInput,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
#[non_exhaustive]
pub enum SigningInput {
    Bytes(Base64urlUInt),
    #[cfg(feature = "eip")]
    TypedData(eip712::TypedData),
    #[serde(rename_all = "camelCase")]
    EthereumPersonalMessage {
        ethereum_personal_message: String,
    },
    Micheline {
        micheline: String,
    },
}

impl ProofPreparation {
    pub async fn complete(self, signature: &str) -> Result<Proof, Error> {
        let proof_type = self.proof.type_.clone();
        let suite = get_proof_suite(&proof_type)?;
        suite.complete(self, signature).await
    }
}

fn use_eip712sig(key: &JWK) -> bool {
    // deprecated: allow using unregistered "signTypedData" key operation value to indicate using EthereumEip712Signature2021
    if let Some(ref key_ops) = key.key_operations {
        if key_ops.contains(&"signTypedData".to_string()) {
            return true;
        }
    }
    false
}

fn use_epsig(key: &JWK) -> bool {
    // deprecated: allow using unregistered "signPersonalMessage" key operation value to indicate using EthereumPersonalSignature2021
    if let Some(ref key_ops) = key.key_operations {
        if key_ops.contains(&"signPersonalMessage".to_string()) {
            return true;
        }
    }
    false
}

// If a verificationMethod purpose was not provided, pick one. If one was provided,
// verify that it is correct for the given issuer and proof purpose.
pub async fn ensure_or_pick_verification_relationship(
    options: &mut LinkedDataProofOptions,
    document: &(dyn LinkedDataDocument + Sync),
    key: &JWK,
    resolver: &dyn DIDResolver,
) -> Result<(), Error> {
    let issuer = match document.get_issuer() {
        None => {
            // No issuer: no check is done.
            // TODO: require issuer - or invokers set for ZCap
            return Ok(());
        }
        Some(issuer) => issuer,
    };
    if options.proof_purpose.is_none() {
        options.proof_purpose = document.get_default_proof_purpose();
    }
    let proof_purpose = options
        .proof_purpose
        .as_ref()
        .ok_or(Error::MissingProofPurpose)?
        .clone();
    if !issuer.starts_with("did:") {
        // TODO: support non-DID issuers.
        // Unable to verify verification relationship for non-DID issuers.
        // Allow some for testing purposes only.
        match issuer {
            #[cfg(feature = "example-http-issuer")]
            "https://example.edu/issuers/14" => {
                // https://github.com/w3c/vc-test-suite/blob/cdc7835/test/vc-data-model-1.0/input/example-016-jwt.jsonld#L8
                // We don't have a way to actually resolve this to anything. Just allow it for
                // vc-test-suite for now.
                return Ok(());
            }
            _ => {
                return Err(Error::UnsupportedNonDIDIssuer(issuer.to_string()));
            }
        }
    }
    if let Some(URI::String(ref vm_id)) = options.verification_method {
        ensure_verification_relationship(issuer, proof_purpose, vm_id, key, resolver).await?;
    } else {
        options.verification_method = Some(URI::String(
            pick_default_vm(issuer, proof_purpose, key, resolver).await?,
        ))
    }
    Ok(())
}

// Ensure a verification relationship exists between a given issuer and verification method for a
// given proof purpose, and that the given JWK is matches the given verification method.
async fn ensure_verification_relationship(
    issuer: &str,
    proof_purpose: ProofPurpose,
    vm: &str,
    jwk: &JWK,
    resolver: &dyn DIDResolver,
) -> Result<(), Error> {
    let vmms =
        ssi_dids::did_resolve::get_verification_methods(issuer, proof_purpose.clone(), resolver)
            .await?;
    let vmm = vmms.get(vm).ok_or_else(|| {
        Error::MissingVerificationRelationship(issuer.to_string(), proof_purpose, vm.to_string())
    })?;
    vmm.match_jwk(jwk)?;
    Ok(())
}

async fn pick_default_vm(
    issuer: &str,
    proof_purpose: ProofPurpose,
    jwk: &JWK,
    resolver: &dyn DIDResolver,
) -> Result<String, Error> {
    let vm_ids =
        ssi_dids::did_resolve::get_verification_methods(issuer, proof_purpose.clone(), resolver)
            .await?;
    let mut err = Error::MissingKey;
    for (vm_id, vmm) in vm_ids {
        // Try to find a VM that matches this JWK and controller.
        match vmm.match_jwk(jwk) {
            Ok(()) => {
                // Found appropriate VM.
                return Ok(vm_id);
            }
            Err(e) => err = e.into(),
        }
    }
    // No matching VM found. Return any error encountered.
    Err(err)
}

pub struct LinkedDataProofs;
impl LinkedDataProofs {
    // https://w3c-ccg.github.io/ld-proofs/#proof-algorithm
    pub async fn sign(
        document: &(dyn LinkedDataDocument + Sync),
        options: &LinkedDataProofOptions,
        resolver: &dyn DIDResolver,
        context_loader: &mut ContextLoader,
        key: &JWK,
        extra_proof_properties: Option<Map<String, Value>>,
    ) -> Result<Proof, Error> {
        // Use type property if present
        let suite = if let Some(ref type_) = options.type_ {
            get_proof_suite(type_)?
        }
        // Otherwise pick proof type based on key and options.
        else {
            pick_proof_suite(key, options.verification_method.as_ref())?
        };
        let mut options = options.clone();
        ensure_or_pick_verification_relationship(&mut options, document, key, resolver).await?;
        suite
            .sign(
                document,
                &options,
                resolver,
                context_loader,
                key,
                extra_proof_properties,
            )
            .await
    }

    /// Prepare to create a linked data proof. Given a linked data document, proof options, and JWS
    /// algorithm, calculate the signing input bytes. Returns a [`ProofPreparation`] - the data for the caller to sign, along with data to reconstruct the proof.
    pub async fn prepare(
        document: &(dyn LinkedDataDocument + Sync),
        options: &LinkedDataProofOptions,
        resolver: &dyn DIDResolver,
        context_loader: &mut ContextLoader,
        public_key: &JWK,
        extra_proof_properties: Option<Map<String, Value>>,
    ) -> Result<ProofPreparation, Error> {
        // Use type property if present
        let suite = if let Some(ref type_) = options.type_ {
            get_proof_suite(type_)?
        }
        // Otherwise pick proof type based on key and options.
        else {
            pick_proof_suite(public_key, options.verification_method.as_ref())?
        };
        let mut options = options.clone();
        ensure_or_pick_verification_relationship(&mut options, document, public_key, resolver)
            .await?;
        suite
            .prepare(
                document,
                &options,
                resolver,
                context_loader,
                public_key,
                extra_proof_properties,
            )
            .await
    }

    // https://w3c-ccg.github.io/ld-proofs/#proof-verification-algorithm
    pub async fn verify(
        proof: &Proof,
        document: &(dyn LinkedDataDocument + Sync),
        resolver: &dyn DIDResolver,
        context_loader: &mut ContextLoader,
    ) -> Result<VerificationWarnings, Error> {
        let suite = get_proof_suite(proof.type_.as_str())?;
        suite
            .verify(proof, document, resolver, context_loader)
            .await
    }
}

async fn to_jws_payload(
    document: &(dyn LinkedDataDocument + Sync),
    proof: &Proof,
    context_loader: &mut ContextLoader,
) -> Result<Vec<u8>, Error> {
    let sigopts_dataset = proof
        .to_dataset_for_signing(Some(document), context_loader)
        .await?;
    let doc_dataset = document
        .to_dataset_for_signing(None, context_loader)
        .await?;
    let doc_dataset_normalized = urdna2015::normalize(&doc_dataset)?;
    let doc_normalized = doc_dataset_normalized.to_nquads()?;
    let sigopts_dataset_normalized = urdna2015::normalize(&sigopts_dataset)?;
    let sigopts_normalized = sigopts_dataset_normalized.to_nquads()?;
    let sigopts_digest = sha256(sigopts_normalized.as_bytes());
    let doc_digest = sha256(doc_normalized.as_bytes());
    let data = [
        sigopts_digest.as_ref().to_vec(),
        doc_digest.as_ref().to_vec(),
    ]
    .concat();
    Ok(data)
}

#[allow(clippy::too_many_arguments)]
async fn sign(
    document: &(dyn LinkedDataDocument + Sync),
    options: &LinkedDataProofOptions,
    _resolver: &dyn DIDResolver,
    context_loader: &mut ContextLoader,
    key: &JWK,
    type_: &str,
    algorithm: Algorithm,
    extra_proof_properties: Option<Map<String, Value>>,
) -> Result<Proof, Error> {
    if let Some(key_algorithm) = key.algorithm {
        if key_algorithm != algorithm {
            return Err(Error::JWS(ssi_jws::Error::AlgorithmMismatch));
        }
    }
    let proof = Proof::new(type_)
        .with_options(options)
        .with_properties(extra_proof_properties);
    sign_proof(document, proof, key, algorithm, context_loader).await
}

async fn sign_proof(
    document: &(dyn LinkedDataDocument + Sync),
    mut proof: Proof,
    key: &JWK,
    algorithm: Algorithm,
    context_loader: &mut ContextLoader,
) -> Result<Proof, Error> {
    let message = to_jws_payload(document, &proof, context_loader).await?;
    let jws = ssi_jws::detached_sign_unencoded_payload(algorithm, &message, key)?;
    proof.jws = Some(jws);
    Ok(proof)
}

#[allow(clippy::too_many_arguments)]
async fn sign_nojws(
    document: &(dyn LinkedDataDocument + Sync),
    options: &LinkedDataProofOptions,
    context_loader: &mut ContextLoader,
    key: &JWK,
    type_: &str,
    algorithm: Algorithm,
    context_uri: &str,
    extra_proof_properties: Option<Map<String, Value>>,
) -> Result<Proof, Error> {
    if let Some(key_algorithm) = key.algorithm {
        if key_algorithm != algorithm {
            return Err(Error::JWS(ssi_jws::Error::AlgorithmMismatch));
        }
    }
    let mut proof = Proof::new(type_)
        .with_options(options)
        .with_properties(extra_proof_properties);
    if !document_has_context(document, context_uri)? {
        proof.context = serde_json::json!([context_uri]);
    }
    let message = to_jws_payload(document, &proof, context_loader).await?;
    let sig = ssi_jws::sign_bytes(algorithm, &message, key)?;
    let sig_multibase = multibase::encode(multibase::Base::Base58Btc, sig);
    proof.proof_value = Some(sig_multibase);
    Ok(proof)
}

#[allow(clippy::too_many_arguments)]
async fn prepare(
    document: &(dyn LinkedDataDocument + Sync),
    options: &LinkedDataProofOptions,
    _resolver: &dyn DIDResolver,
    context_loader: &mut ContextLoader,
    public_key: &JWK,
    type_: &str,
    algorithm: Algorithm,
    extra_proof_properties: Option<Map<String, Value>>,
) -> Result<ProofPreparation, Error> {
    if let Some(key_algorithm) = public_key.algorithm {
        if key_algorithm != algorithm {
            return Err(Error::JWS(ssi_jws::Error::AlgorithmMismatch));
        }
    }
    let proof = Proof::new(type_)
        .with_options(options)
        .with_properties(extra_proof_properties);
    prepare_proof(document, proof, algorithm, context_loader).await
}

async fn prepare_proof(
    document: &(dyn LinkedDataDocument + Sync),
    proof: Proof,
    algorithm: Algorithm,
    context_loader: &mut ContextLoader,
) -> Result<ProofPreparation, Error> {
    let message = to_jws_payload(document, &proof, context_loader).await?;
    let (jws_header, signing_input) =
        ssi_jws::prepare_detached_unencoded_payload(algorithm, &message)?;
    Ok(ProofPreparation {
        proof,
        jws_header: Some(jws_header),
        signing_input: SigningInput::Bytes(Base64urlUInt(signing_input)),
    })
}

#[allow(clippy::too_many_arguments)]
async fn prepare_nojws(
    document: &(dyn LinkedDataDocument + Sync),
    options: &LinkedDataProofOptions,
    context_loader: &mut ContextLoader,
    public_key: &JWK,
    type_: &str,
    algorithm: Algorithm,
    context_uri: &str,
    extra_proof_properties: Option<Map<String, Value>>,
) -> Result<ProofPreparation, Error> {
    if let Some(key_algorithm) = public_key.algorithm {
        if key_algorithm != algorithm {
            return Err(Error::JWS(ssi_jws::Error::AlgorithmMismatch));
        }
    }
    let mut proof = Proof::new(type_)
        .with_options(options)
        .with_properties(extra_proof_properties);
    if !document_has_context(document, context_uri)? {
        proof.context = serde_json::json!([context_uri]);
    }
    let message = to_jws_payload(document, &proof, context_loader).await?;
    Ok(ProofPreparation {
        proof,
        jws_header: None,
        signing_input: SigningInput::Bytes(Base64urlUInt(message)),
    })
}

async fn complete(preparation: ProofPreparation, signature: &str) -> Result<Proof, Error> {
    complete_proof(preparation, signature).await
}

async fn complete_proof(preparation: ProofPreparation, signature: &str) -> Result<Proof, Error> {
    let mut proof = preparation.proof;
    let jws_header = preparation.jws_header.ok_or(Error::MissingJWSHeader)?;
    let jws = ssi_jws::complete_sign_unencoded_payload(jws_header, signature)?;
    proof.jws = Some(jws);
    Ok(proof)
}

async fn verify(
    proof: &Proof,
    document: &(dyn LinkedDataDocument + Sync),
    resolver: &dyn DIDResolver,
    context_loader: &mut ContextLoader,
) -> Result<VerificationWarnings, Error> {
    let jws = proof.jws.as_ref().ok_or(Error::MissingProofSignature)?;
    let verification_method = proof
        .verification_method
        .as_ref()
        .ok_or(Error::MissingVerificationMethod)?;
    let key = resolve_key(verification_method, resolver).await?;
    let message = to_jws_payload(document, proof, context_loader).await?;
    ssi_jws::detached_verify(jws, &message, &key)?;
    Ok(Default::default())
}

async fn verify_nojws(
    proof: &Proof,
    document: &(dyn LinkedDataDocument + Sync),
    resolver: &dyn DIDResolver,
    context_loader: &mut ContextLoader,
    algorithm: Algorithm,
) -> Result<VerificationWarnings, Error> {
    let proof_value = proof
        .proof_value
        .as_ref()
        .ok_or(Error::MissingProofSignature)?;
    let verification_method = proof
        .verification_method
        .as_ref()
        .ok_or(Error::MissingVerificationMethod)?;
    let key = resolve_key(verification_method, resolver).await?;
    let message = to_jws_payload(document, proof, context_loader).await?;
    let (_base, sig) = multibase::decode(proof_value)?;
    Ok(ssi_jws::verify_bytes_warnable(
        algorithm, &message, &key, &sig,
    )?)
}

// Check if a linked data document has a given URI in its @context array.
fn document_has_context(
    document: &(dyn LinkedDataDocument + Sync),
    context_uri: &str,
) -> Result<bool, Error> {
    let contexts_string = document.get_contexts()?.ok_or(Error::MissingContext)?;
    let contexts: ssi_core::one_or_many::OneOrMany<Context> =
        serde_json::from_str(&contexts_string)?;
    Ok(contexts.into_iter().any(|c| match c {
        Context::URI(URI::String(u)) if u == context_uri => true,
        _ => false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssi_dids::example::DIDExample;
    use ssi_json_ld::CREDENTIALS_V1_CONTEXT;

    struct ExampleDocument;

    #[async_trait]
    impl LinkedDataDocument for ExampleDocument {
        fn get_contexts(&self) -> Result<Option<String>, Error> {
            Ok(Some(serde_json::to_string(&*CREDENTIALS_V1_CONTEXT)?))
        }
        async fn to_dataset_for_signing(
            &self,
            _parent: Option<&(dyn LinkedDataDocument + Sync)>,
            _context_loader: &mut ContextLoader,
        ) -> Result<DataSet, Error> {
            use ssi_json_ld::rdf;
            let mut dataset = DataSet::default();
            let statement = rdf::Statement {
                subject: rdf::Subject::BlankNodeLabel(rdf::BlankNodeLabel("_:c14n0".to_string())),
                predicate: rdf::Predicate::IRIRef(rdf::IRIRef(
                    "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),
                )),
                object: rdf::Object::IRIRef(rdf::IRIRef(
                    "http://example.org/vocab#Foo".to_string(),
                )),
                graph_label: None,
            };
            dataset.add_statement(statement);
            Ok(dataset)
        }

        fn to_value(&self) -> Result<Value, Error> {
            Err(Error::MissingAlgorithm)
        }
    }

    #[cfg(feature = "eip")]
    #[async_std::test]
    async fn eip712vm() {
        let mut key = JWK::generate_secp256k1().unwrap();
        key.algorithm = Some(Algorithm::ES256KR);
        let vm = format!("{}#Recovery2020", "did:example:foo");
        let issue_options = LinkedDataProofOptions {
            verification_method: Some(URI::String(vm)),
            ..Default::default()
        };
        let resolver = DIDExample;
        let mut context_loader = ssi_json_ld::ContextLoader::default();
        let doc = ExampleDocument;
        let _proof = LinkedDataProofs::sign(
            &doc,
            &issue_options,
            &resolver,
            &mut context_loader,
            &key,
            None,
        )
        .await
        .unwrap();
    }

    #[async_std::test]
    #[cfg(feature = "tezos")]
    async fn tezos_vm_tz1() {
        let mut key = JWK::generate_ed25519().unwrap();
        key.algorithm = Some(Algorithm::EdBlake2b);
        let vm = format!("{}#TezosMethod2021", "did:example:foo");
        let issue_options = LinkedDataProofOptions {
            type_: Some(String::from("TezosSignature2021")),
            verification_method: Some(URI::String(vm)),
            ..Default::default()
        };
        let doc = ExampleDocument;
        let resolver = DIDExample;
        let mut context_loader = ssi_json_ld::ContextLoader::default();
        let proof = LinkedDataProofs::sign(
            &doc,
            &issue_options,
            &resolver,
            &mut context_loader,
            &key,
            None,
        )
        .await
        .unwrap();
        println!("{}", serde_json::to_string(&proof).unwrap());
        // TODO: verify
    }

    #[async_std::test]
    #[cfg(feature = "tezos")]
    async fn tezos_vm_tz2() {
        let mut key = JWK::generate_secp256k1().unwrap();
        key.algorithm = Some(Algorithm::ESBlake2bK);
        let vm = format!("{}#TezosMethod2021", "did:example:foo");
        let issue_options = LinkedDataProofOptions {
            type_: Some(String::from("TezosSignature2021")),
            verification_method: Some(URI::String(vm)),
            ..Default::default()
        };
        let doc = ExampleDocument;
        let resolver = DIDExample;
        let mut context_loader = ssi_json_ld::ContextLoader::default();
        let proof = LinkedDataProofs::sign(
            &doc,
            &issue_options,
            &resolver,
            &mut context_loader,
            &key,
            None,
        )
        .await
        .unwrap();
        println!("{}", serde_json::to_string(&proof).unwrap());
    }

    #[async_std::test]
    #[cfg(feature = "tezos")]
    async fn tezos_jcs_vm_tz2() {
        let mut key = JWK::generate_secp256k1().unwrap();
        key.algorithm = Some(Algorithm::ESBlake2bK);
        let vm = format!("{}#TezosMethod2021", "did:example:foo");
        let issue_options = LinkedDataProofOptions {
            type_: Some(String::from("TezosJcsSignature2021")),
            verification_method: Some(URI::String(vm)),
            ..Default::default()
        };
        let doc = ExampleDocument;
        let resolver = DIDExample;
        let mut context_loader = ssi_json_ld::ContextLoader::default();
        let proof = LinkedDataProofs::sign(
            &doc,
            &issue_options,
            &resolver,
            &mut context_loader,
            &key,
            None,
        )
        .await
        .unwrap();
        println!("{}", serde_json::to_string(&proof).unwrap());
    }

    /*
    #[async_std::test]
    async fn solvm() {
        let mut key = JWK::generate_secp256k1().unwrap();
        key.algorithm = Some(Algorithm::ES256KR);
        let vm = format!("{}#SolanaMethod2021", "did:example:foo");
        let issue_options = LinkedDataProofOptions {
            verification_method: Some(vm),
            ..Default::default()
        };
        let doc = ExampleDocument;
        let resolver = DIDExample;
        let _proof = LinkedDataProofs::sign(&doc, &issue_options, &resolver, &key)
            .await
            .unwrap();
    }
    */
}
