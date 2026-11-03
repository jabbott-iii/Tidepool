# Third-party test data

The files here are not the project's own, so each keeps its notice beside
it (ADR-0021, as the RFC 8259 examples do in
`crates/codec/tests/json.rs`).

## `codeql-template.yml.txt`

GitHub's "CodeQL Advanced" starter workflow, as the user committed it on
2026-11-01 (`git show ed8edf4:.github/workflows/codeql.yml`), byte for
byte. GitHub generated it from `code-scanning/codeql.yml` in
[actions/starter-workflows](https://github.com/actions/starter-workflows),
filling in the branch, the weekly schedule and the languages. The
`workflows` tests in `xtask` read it as the template M0.8 replaced. Its
license, from that repository's `LICENSE`:

```text
MIT License

Copyright (c) 2020 GitHub

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE. THIS LICENSE DOES NOT GRANT YOU RIGHTS TO USE ANY CONTRIBUTORS'
NAME, LOGO, OR TRADEMARKS.
```
