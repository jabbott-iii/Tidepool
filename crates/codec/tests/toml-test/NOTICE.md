# Third-party test data: toml-test

The files here are not the project's own, so they keep their notice beside
them (ADR-0021, ADR-0026).

They are the TOML 1.1 cases of
[toml-lang/toml-test](https://github.com/toml-lang/toml-test), release
v2.2.0 (commit `ce08da1ddb075d1c7596d663c7fcba9a2ae02c5c`): the list
`tests/files-toml-1.1.0`, here `files-toml-1.1.0`, and the 895 files it
names, under `valid/` and `invalid/`, byte for byte. Each file's git blob
hash was checked against that commit's tree when it was copied, on
2026-11-04. Nothing else from the repository is here: its Go runner's
comparison rules are written again in `crates/codec/tests/toml.rs`.

They are test-only and never in a release binary. `.gitattributes` keeps
their bytes, since some cases are about CR, BOMs and invalid UTF-8. To
take a newer release, replace this directory and this notice in one
change.

Their license, from that repository's `LICENSE`:

```text
The MIT License (MIT)

Copyright (c) 2018 TOML authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```
