# Third-Party Notices

Workman depends on open-source software listed in `Cargo.lock` and the npm lockfiles. Release
archives include a generated copy of this notice followed by the license files shipped by those
exact dependency versions.

## shadcn-svelte

Parts of `apps/desktop/src/lib/components/ui` were generated from or adapted from
[shadcn-svelte](https://github.com/huntabyte/shadcn-svelte).

MIT License

Copyright (c) 2023 Hunter Johnston <https://github.com/huntabyte>

Copyright (c) 2023 CokaKoala <https://github.com/adriangonz97>

Copyright (c) 2023 shadcn

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge, publish, distribute,
sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT
OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## Generated Cloudflare Workers types

`infra/update-host/worker-configuration.d.ts` is generated from Cloudflare Workers types and
retains its upstream Apache-2.0 attribution and license header in the generated file.

## Refreshing packaged notices

Run `node scripts/generate-third-party-notices.mjs <output-path>` after installing the locked Cargo
and desktop npm dependencies. The release scripts run it automatically.
