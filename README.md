# lights-out

A generalised [Lights Out](https://en.wikipedia.org/wiki/Lights_Out_(game)) generator and solver, written in Rust and compiled to WebAssembly.

- Lights cycle through `k` states (`2 <= k <= 255`), not just on/off.
- Closed or open neighbourhood presses, king's move presses, knight's move, row + column.
- Grid, cylinder, Möbius strip, torus, Klein bottle, real projective plane, sphere, plus 1-D paths and cycles.

Try it on the blog: <https://blog.navalcitrus.net/posts/lights-out/>

## How it works

A board is a vector over `\mathbb{Z}/k\mathbb{Z}` and pressing cell `j` adds a fixed column `M_j`, so solving is the linear system `M a \equiv -s \pmod{k}`.
The solver factors `k`, solves modulo each prime power by Gauss--Jordan elimination over `\mathbb{F}_p` followed by Hensel lifting, then recombines with the Chinese Remainder Theorem.

Each 2-D surface is the `rows \times cols` rectangle as a fundamental domain for a group of isometries of `\mathbb{Z}^2`, except the sphere which collapses the boundary to a point.

## Layout

| Path                   | Contents                                                              |
| :--------------------- | :-------------------------------------------------------------------- |
| `crates/core`          | Topologies, quotients, press patterns, the solver.                    |
| `crates/wasm`          | `wasm-bindgen` surface used by the web front end.                     |
| `web/index.ts`         | TypeScript front end, mounted on every `.lights-out` element.         |
| `web/lights-out.css`   | Styles for the front end, built on the `naval-citrus` CSS variables.  |

## Building

Everything goes through the Nix flake (`direnv allow` or `nix develop` for a shell).

```sh
nix flake check   # clippy, tests, and the web bundle (which type-checks index.ts)
nix build         # the web bundle, in ./result
```

`result/` mirrors the Hugo site:

```text
result/
├── assets/css/lights-out.css
└── static/js/lights-out/
    ├── index.js
    └── pkg/
        ├── lights_out_wasm.js
        └── lights_out_wasm_bg.wasm
```

## Deploying to the blog

From the root of the blog's Hugo site, copy the bundle over:

```sh
cp -r --no-preserve=mode /path/to/lights-out/result/. .
```

The blog then needs:

- `css/lights-out.css` listed in `params.extraStyles` in `hugo.toml`.
- `wasm: true` in the post's front matter, which adds `'wasm-unsafe-eval'` to that page's Content Security Policy.
- The mount point and script in the post:

  ```html
  {{<rawhtml>}}
  <div class="lights-out panel">Loading Lights Out...</div>
  <script type="module" src="/js/lights-out/index.js"></script>
  {{</rawhtml>}}
  ```
