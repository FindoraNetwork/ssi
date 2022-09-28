[![](https://img.shields.io/badge/Rust-v1.63.0-orange)](https://www.rust-lang.org/)
[![](https://img.shields.io/badge/License-Apache--2.0-green)](https://github.com/spruceid/didkit/blob/main/LICENSE)

SSI's documentation is currently packaged with the DIDKit documentation
[here](https://spruceid.dev/docs/didkit/).

# SSI

SSI provides core Verifiable Credential and Decentralized Identifier
functionality in Rust. Rust was chosen for its expressive type system, memory
safety, simple dependency web, and suitability across different platforms
including embedded systems. This library is embedded in the the cross-platform
[`didkit`](https://github.com/FindoraNetwork/didkit) library as a core dependency.

![DIDKit core components](https://user-images.githubusercontent.com/37127325/132885372-9cdf586e-ba6f-44c8-8b83-f72f16d86107.png)

## Security Audits

ssi has undergone the following security reviews:
- [March 14th, 2022 - Trail of Bits](https://github.com/trailofbits/publications/blob/master/reviews/SpruceID.pdf) | [Summary of Findings](https://blog.spruceid.com/spruce-completes-first-security-audit-from-trail-of-bits/)


## Dependencies

```
clang
openssl-devel
```

## Install

### Crates.io

```
ssi = "0.4"
```

### From Source

```sh
$ git clone https://github.com/FindoraNetwork/ssi
$ cd ssi
$ git submodule update --init
$ cargo build
```

## Additional resources

- [Rust](https://www.rust-lang.org/)
- [rustup](https://rustup.rs/)
- [Cargo](https://doc.rust-lang.org/cargo/)
