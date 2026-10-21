# Third-party notices

BenchLab itself is licensed under Apache-2.0; see [LICENSE](LICENSE).

## Direct dependencies

| Component | License | Use |
| --- | --- | --- |
| [GPUI](https://github.com/zed-industries/zed) 0.2.2 | Apache-2.0 | Native UI framework |
| [unicode-segmentation](https://crates.io/crates/unicode-segmentation) | MIT OR Apache-2.0 | Grapheme-aware text editing |
| [serde](https://crates.io/crates/serde) / [serde_json](https://crates.io/crates/serde_json) | MIT OR Apache-2.0 | Serialization |
| [uuid](https://crates.io/crates/uuid) | Apache-2.0 OR MIT | Stable identifiers |
| [chrono](https://crates.io/crates/chrono) | MIT OR Apache-2.0 | Timestamps |
| [anyhow](https://crates.io/crates/anyhow) / [thiserror](https://crates.io/crates/thiserror) | MIT OR Apache-2.0 | Error handling |

A complete dependency inventory will be generated for release artifacts. Until
then, `Cargo.lock` is the authoritative resolved dependency list.

## External tools

### k6

BenchLab discovers and invokes a user-installed **k6** binary. k6 is licensed
under **AGPL-3.0**. It is not linked into BenchLab and is not redistributed by
BenchLab. Users install it separately; see the k6 project for source and license
terms.

### Fonts and icons

The M0 shell uses the operating system's default UI font and no icon assets.
Bundled fonts and Lucide icons will be added with their license texts when they
are introduced.
