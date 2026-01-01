import init, { Puzzle, TopologyKind, PressKind } from "./pkg/lights_out_wasm.js";

type TopologyName = keyof typeof TopologyKind;
type PressName = keyof typeof PressKind;

const TOPOLOGIES: [TopologyName, string][] = [
    ["Grid", "Grid (open)"],
    ["Torus", "Torus (wraps both)"],
    ["Cylinder", "Cylinder (wraps rows)"],
    ["Mobius", "Möbius strip"],
    ["Klein", "Klein bottle"],
    ["Projective", "Projective plane"],
    ["Sphere", "Sphere (boundary -> point)"],
    ["Path", "Path (1-D, open)"],
    ["Cycle", "Cycle (1-D, wraps)"],
];

const PRESSES: [PressName, string][] = [
    ["Closed", "Closed (self + neighbours)"],
    ["Open", "Open (neighbours only)"],
    ["Kings", "King's move (2-D only)"],
    ["Knights", "Knight's move (2-D only)"],
    ["RowColumn", "Row + column (2-D only)"],
];

// Ways of drawing the gluing, selectable from the preview panel.
type PreviewStyle = "arcs" | "arrows" | "ring";
const PREVIEW_STYLES: [PreviewStyle, string][] = [
    ["arcs", "Wrap arcs"],
    ["arrows", "Edge arrows"],
    ["ring", "Neighbour ring"],
];

const ONE_D_TOPOLOGIES = new Set<string>(["Path", "Cycle"]);
const TWO_D_PRESSES = new Set<string>(["Kings", "Knights", "RowColumn"]);
const SVG_NS = "http://www.w3.org/2000/svg";

// Edges longer than this in layout space are rendered as bowed arcs that sweep outward from the layout centre.
const WRAP_THRESHOLD = 1.6;
const NODE_R = 0.18;

type Attrs = Record<string, string | number>;

const setAttrs = <E extends Element>(el: E, attrs: Attrs): E => {
    for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
    return el;
};

const h = <K extends keyof HTMLElementTagNameMap>(
    tag: K,
    attrs: Attrs = {},
    ...children: (Node | string)[]
): HTMLElementTagNameMap[K] => {
    const node = setAttrs(document.createElement(tag), attrs);
    node.append(...children);
    return node;
};

const select = (options: [string, string][]): HTMLSelectElement =>
    h("select", {}, ...options.map(([value, label]) => h("option", { value }, label)));

const number = (min: number, max: number, value: number): HTMLInputElement =>
    h("input", { type: "number", min, max, value });

const clamp = (n: number, lo: number, hi: number): number =>
    Number.isFinite(n) ? Math.max(lo, Math.min(hi, n)) : lo;

const readInt = (el: HTMLInputElement): number =>
    clamp(parseInt(el.value, 10), parseInt(el.min, 10), parseInt(el.max, 10));

const errorText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

const plural = (n: number, word: string): string => `${n} ${word}${n === 1 ? "" : "s"}`;

const cellColor = (v: number, k: number): string => {
    if (v === 0) return "var(--lo-cell-off)";
    const hue = ((v - 1) / Math.max(1, k - 1)) * 330;
    return `hsl(${hue.toFixed(1)}, 70%, 55%)`;
};

// Unbiased uniform integers in `[0, k)`: bytes at or above the largest multiple of `k` are rejected and redrawn.
const randomCounts = (n: number, k: number): Uint8Array => {
    const limit = Math.floor(256 / k) * k;
    const byte = new Uint8Array(1);
    const draw = (): number => {
        do crypto.getRandomValues(byte); while (byte[0] >= limit);
        return byte[0] % k;
    };
    return Uint8Array.from({ length: n }, draw);
};

// Draws into an SVG in layout coordinates and grows the view box to fit everything drawn.
class Canvas {
    private minX = Infinity;
    private minY = Infinity;
    private maxX = -Infinity;
    private maxY = -Infinity;

    constructor(private readonly svg: SVGSVGElement) {
        svg.replaceChildren();
    }

    include(x: number, y: number, r = 0): void {
        this.minX = Math.min(this.minX, x - r);
        this.minY = Math.min(this.minY, y - r);
        this.maxX = Math.max(this.maxX, x + r);
        this.maxY = Math.max(this.maxY, y + r);
    }

    add(tag: string, attrs: Attrs, text?: string): void {
        const el = setAttrs(document.createElementNS(SVG_NS, tag), attrs);
        if (text !== undefined) el.textContent = text;
        this.svg.appendChild(el);
    }

    node(cls: string, x: number, y: number, r: number, label?: string): void {
        this.include(x, y, r);
        this.add("circle", { class: cls, cx: x, cy: y, r });
        if (label !== undefined) {
            this.add("text", { class: `${cls}-label`, x, y, "font-size": r * 1.1 }, label);
        }
    }

    line(cls: string, ax: number, ay: number, bx: number, by: number): void {
        this.add("line", { class: cls, x1: ax, y1: ay, x2: bx, y2: by });
    }

    finish(pad: number): void {
        if (!Number.isFinite(this.minX)) return;
        const w = Math.max(0.001, this.maxX - this.minX + 2 * pad);
        const hgt = Math.max(0.001, this.maxY - this.minY + 2 * pad);
        this.svg.setAttribute("viewBox", `${this.minX - pad} ${this.minY - pad} ${w} ${hgt}`);
    }
}

type Point = [number, number];

// A raw grid step `(dr, dc)` from `src` that crosses a glued edge and lands on `dst`.
interface Seam {
    src: number;
    dr: number;
    dc: number;
    dst: number;
}

// Everything the preview needs, read once from the puzzle.
interface PreviewData {
    pos: Point[];
    edges: [number, number][];
    // Each ghost's position and the canonical cell it stands for.
    ghosts: { at: Point; cell: number }[];
    bounds: Float32Array;
    rows: number;
    cols: number;
    // Canonical cell of each raw grid position.
    canon: Uint32Array;
    seams: Seam[];
}

// Split a flat array into consecutive tuples of `size` entries.
const chunks = <T extends ArrayLike<number>>(flat: T, size: number): number[][] =>
    Array.from({ length: flat.length / size }, (_, i) =>
        Array.from({ length: size }, (_, j) => flat[size * i + j]));

const readPreview = (puzzle: Puzzle): PreviewData => {
    const positions = chunks(puzzle.layoutPositions(), 2) as Point[];
    const ghostXY = chunks(puzzle.layoutGhostXY(), 2) as Point[];
    const ghostCells = puzzle.layoutGhostCells();
    return {
        pos: positions,
        edges: chunks(puzzle.adjacencyEdges(), 2) as [number, number][],
        ghosts: ghostXY.map((at, i) => ({ at, cell: ghostCells[i] })),
        bounds: puzzle.layoutBounds(),
        rows: puzzle.visualRows,
        cols: puzzle.visualCols,
        canon: puzzle.visualLayout(),
        seams: chunks(puzzle.seamSteps(), 4).map(([src, dr, dc, dst]) => ({ src, dr, dc, dst })),
    };
};

const isWrap = (d: PreviewData, [a, b]: [number, number]): boolean => {
    const [ax, ay] = d.pos[a];
    const [bx, by] = d.pos[b];
    return Math.hypot(bx - ax, by - ay) > WRAP_THRESHOLD;
};

// What every style shares: the non-wrapping edges, collapsed preimages (the sphere's pole) as faint ghosts tied back to their canonical cell, then the cells on top.
const drawBoard = (c: Canvas, d: PreviewData, r = NODE_R, numbered = false): void => {
    for (const [a, b] of d.edges.filter((e) => !isWrap(d, e))) c.line("edge", ...d.pos[a], ...d.pos[b]);
    for (const { at, cell } of d.ghosts) c.line("glue", ...at, ...d.pos[cell]);
    for (const { at } of d.ghosts) c.node("ghost", ...at, NODE_R * 0.75);
    d.pos.forEach((p, i) => c.node("node", ...p, r, numbered ? String(i + 1) : undefined));
};

const drawArcs = (c: Canvas, d: PreviewData): void => {
    const [cx, cy] = [(d.bounds[0] + d.bounds[2]) / 2, (d.bounds[1] + d.bounds[3]) / 2];
    for (const [a, b] of d.edges.filter((e) => isWrap(d, e))) {
        const [ax, ay] = d.pos[a];
        const [bx, by] = d.pos[b];
        const [mx, my] = [(ax + bx) / 2, (ay + by) / 2];
        const nlen = Math.hypot(mx - cx, my - cy) || 1;
        const bow = 0.35 * Math.hypot(bx - ax, by - ay);
        const px = mx + ((mx - cx) / nlen) * bow;
        const py = my + ((my - cy) / nlen) * bow;
        c.include(px, py);
        c.add("path", { class: "wrap", d: `M ${ax} ${ay} Q ${px} ${py} ${bx} ${by}` });
    }
    drawBoard(c, d);
};

type Side = "top" | "bottom" | "left" | "right";

const sideOf = (dr: number, dc: number): Side =>
    dr < 0 ? "top" : dr > 0 ? "bottom" : dc < 0 ? "left" : "right";

const SIDES: Side[] = ["top", "left", "bottom", "right"];

// Where a raw cell sits along the side it touches, increasing left to right or top to bottom.
const along = (d: PreviewData, raw: number, side: Side): number =>
    side === "top" || side === "bottom" ? raw % d.cols : Math.floor(raw / d.cols);

// The side a seam step lands on: the one facing back the way it came.
const landing = (d: PreviewData, { dr, dst }: Seam): Side =>
    dr !== 0
        ? (Math.floor(dst / d.cols) === 0 ? "top" : "bottom")
        : (dst % d.cols === 0 ? "left" : "right");

// How each side is glued: the partner side, and whether the gluing reverses direction.
// Seams come in raw cell order, so along each side they run in increasing `along` order.
const gluings = (d: PreviewData): Map<Side, { to: Side; reversed: boolean }> => {
    const out = new Map<Side, { to: Side; reversed: boolean }>();
    for (const side of SIDES) {
        const seams = d.seams.filter(({ dr, dc }) => sideOf(dr, dc) === side);
        if (seams.length === 0) continue;
        const [first, last] = [seams[0], seams[seams.length - 1]];
        const to = landing(d, first);
        out.set(side, { to, reversed: along(d, last.dst, to) < along(d, first.dst, to) });
    }
    return out;
};

// Textbook fundamental polygon: the rectangle's outline with matching arrows on glued sides.
// One chevron marks the first pair of sides and two chevrons the second, and the arrow direction says which way round they are glued.
const drawArrows = (c: Canvas, d: PreviewData): void => {
    const [x0, y0, x1, y1] = [-0.5, -0.5, d.cols - 0.5, d.rows - 0.5];
    const ends: Record<Side, [Point, Point]> = {
        top: [[x0, y0], [x1, y0]],
        bottom: [[x0, y1], [x1, y1]],
        left: [[x0, y0], [x0, y1]],
        right: [[x1, y0], [x1, y1]],
    };
    const glued = gluings(d);
    const marks = new Map<Side, { count: number; reversed: boolean }>();
    let pair = 0;
    for (const side of SIDES) {
        const g = glued.get(side);
        if (!g || marks.has(side)) continue;
        pair++;
        marks.set(side, { count: pair, reversed: false });
        marks.set(g.to, { count: pair, reversed: g.reversed });
    }

    for (const side of SIDES) {
        const [[ax, ay], [bx, by]] = ends[side];
        c.include(ax, ay);
        c.include(bx, by);
        const mark = marks.get(side);
        c.line(mark ? "seam" : "border", ax, ay, bx, by);
        if (!mark) continue;
        // Chevrons sit at the middle of the side and point along it.
        const [ux, uy] = mark.reversed ? [ax - bx, ay - by] : [bx - ax, by - ay];
        const len = Math.hypot(ux, uy);
        const [tx, ty] = [ux / len, uy / len];
        const [nx, ny] = [-ty, tx];
        const s = 0.22;
        for (let k = 0; k < mark.count; k++) {
            const off = (k - (mark.count - 1) / 2) * s * 1.1;
            const [px, py] = [(ax + bx) / 2 + tx * off, (ay + by) / 2 + ty * off];
            c.add("path", {
                class: "chevron",
                d: `M ${px - tx * s + nx * s} ${py - ty * s + ny * s} L ${px} ${py} L ${px - tx * s - nx * s} ${py - ty * s - ny * s}`,
            });
        }
    }
    drawBoard(c, d);
};

// One ring of ghost cells around the board, each numbered with the real cell you reach by stepping off that edge.
const drawRing = (c: Canvas, d: PreviewData): void => {
    const label = (raw: number): string => String(d.canon[raw] + 1);
    const r = NODE_R * 1.5;
    const xy = (raw: number): Point => [raw % d.cols, Math.floor(raw / d.cols)];
    for (const { src, dr, dc } of d.seams) {
        const [sx, sy] = xy(src);
        c.line("ring-link", sx, sy, sx + dc, sy + dr);
    }
    for (const { src, dr, dc, dst } of d.seams) {
        const [sx, sy] = xy(src);
        c.node("ghost-cell", sx + dc, sy + dr, r, label(dst));
    }
    drawBoard(c, d, r, true);
};

const buildUi = (root: Element) => {
    const ui = {
        topology: select(TOPOLOGIES),
        press: select(PRESSES),
        modulus: number(2, 255, 2),
        rows: number(3, 15, 5),
        cols: number(3, 15, 5),
        scramble: h("button", { type: "button" }, "Scramble"),
        solve: h("button", { type: "button" }, "Solve"),
        reset: h("button", { type: "button" }, "Reset"),
        status: h("p", { class: "lo-status", role: "status", "aria-live": "polite" }),
        board: h("div", { class: "lo-board", role: "grid", "aria-label": "Lights-out board" }),
        previewStyle: select(PREVIEW_STYLES),
        preview: setAttrs(document.createElementNS(SVG_NS, "svg"), {
            viewBox: "0 0 100 100",
            preserveAspectRatio: "xMidYMid meet",
        }),
    };

    const label = (text: string, input: HTMLElement) => h("label", {}, text, input);
    root.replaceChildren(
        h("section", { class: "lo-controls", "aria-label": "Puzzle settings" },
            label("Topology", ui.topology),
            label("Press", ui.press),
            label("Modulus k", ui.modulus),
            label("Rows", ui.rows),
            label("Cols", ui.cols),
            ui.scramble, ui.solve, ui.reset),
        ui.status,
        h("div", { class: "lo-board-wrap" }, ui.board),
        h("section", { class: "lo-preview", "aria-label": "Topology preview" },
            h("div", { class: "lo-preview-head" }, h("h3", {}, "Topology"), ui.previewStyle),
            ui.preview),
    );
    return ui;
};

const mount = (root: Element): void => {
    const ui = buildUi(root);
    let puzzle: Puzzle | null = null;
    let state: Uint8Array = new Uint8Array();
    let solveQueue: number[] = [];

    const setStatus = (text: string, kind = ""): void => {
        ui.status.textContent = text;
        ui.status.className = kind ? `lo-status ${kind}` : "lo-status";
    };

    // Run a wasm call, reporting any error in the status line instead of throwing.
    const attempt = <T>(f: () => T): T | undefined => {
        try {
            return f();
        } catch (err) {
            setStatus(errorText(err), "error");
        }
    };

    const rebuild = (): void => {
        const is1D = ONE_D_TOPOLOGIES.has(ui.topology.value);
        for (const opt of ui.press.options) opt.disabled = is1D && TWO_D_PRESSES.has(opt.value);
        // "Closed" works on every topology.
        if (ui.press.selectedOptions[0]?.disabled) ui.press.value = "Closed";
        ui.cols.disabled = is1D;

        const topology = TopologyKind[ui.topology.value as TopologyName];
        const press = PressKind[ui.press.value as PressName];
        const k = readInt(ui.modulus);
        const rows = readInt(ui.rows);
        const cols = is1D ? 1 : readInt(ui.cols);

        puzzle?.free();
        puzzle = attempt(() => Puzzle.build(topology, press, rows, cols, k)) ?? null;
        if (!puzzle) {
            ui.board.replaceChildren();
            ui.preview.replaceChildren();
            return;
        }

        state = puzzle.zeroState();
        solveQueue = [];

        renderGrid(puzzle);
        renderPreview();
        paint();
        setStatus("Ready. Click cells, or Scramble to start a puzzle.");
    };

    const renderPreview = (): void => {
        if (!puzzle) return;
        const d = readPreview(puzzle);
        const c = new Canvas(ui.preview);
        if (d.pos.length === 0) return;
        // 1-D boards have no rectangle to glue, so only the arc style applies.
        const style = ONE_D_TOPOLOGIES.has(ui.topology.value)
            ? "arcs"
            : (ui.previewStyle.value as PreviewStyle);
        ({ arcs: drawArcs, arrows: drawArrows, ring: drawRing })[style](c, d);
        c.finish(NODE_R + 0.05);
    };

    const renderGrid = (p: Puzzle): void => {
        ui.board.replaceChildren();
        ui.board.style.setProperty("--lo-cols", String(p.visualCols));
        for (const cellId of p.visualLayout()) {
            const cell = h("button", { type: "button", class: "lo-cell", "data-cell-id": cellId });
            cell.addEventListener("click", () => onCellClick(cellId));
            ui.board.appendChild(cell);
        }
    };

    const paint = (): void => {
        if (!puzzle) return;
        const k = puzzle.modulus;
        const remaining = new Map<number, number>();
        for (const mv of solveQueue) remaining.set(mv, (remaining.get(mv) ?? 0) + 1);
        const nextMove = solveQueue[0] ?? null;

        for (const cell of ui.board.children as HTMLCollectionOf<HTMLButtonElement>) {
            const cellId = Number(cell.dataset.cellId);
            const v = state[cellId];
            const count = remaining.get(cellId) ?? 0;

            cell.style.background = cellColor(v, k);
            cell.textContent = v === 0 ? "" : String(v);
            cell.classList.toggle("hint", count > 0);
            cell.classList.toggle("hint-next", cellId === nextMove);
            if (count > 1) cell.dataset.hintCount = String(count);
            else delete cell.dataset.hintCount;
        }

        if (puzzle.isSolved(state)) setStatus("Solved!", "solved");
    };

    const onCellClick = (moveId: number): void => {
        const p = puzzle;
        const next = p && attempt(() => p.apply(state, moveId));
        if (!next) return;
        state = next;
        advancePlan(moveId);
        paint();
    };

    const advancePlan = (moveId: number): void => {
        if (solveQueue.length === 0) {
            setStatus("");
            return;
        }
        if (solveQueue[0] !== moveId) {
            solveQueue = [];
            setStatus("Pressed cell off plan: hints cleared.");
            return;
        }
        solveQueue.shift();
        const left = solveQueue.length;
        setStatus(left === 0 ? "Plan complete." : `Following plan (${plural(left, "move")} left).`);
    };

    const scramble = (): void => {
        const p = puzzle;
        const next = p && attempt(() => p.scramble(randomCounts(p.moveCount, p.modulus)));
        if (!next) return;
        state = next;
        solveQueue = [];
        setStatus("Scrambled. Solve it!");
        paint();
    };

    const reset = (): void => {
        if (!puzzle) return;
        state = puzzle.zeroState();
        solveQueue = [];
        setStatus("Reset.");
        paint();
    };

    const solve = (): void => {
        const p = puzzle;
        if (!p) return;
        const plan = attempt(() => p.solve(state, p.zeroState()) ?? null);
        if (plan === undefined) return;
        solveQueue = plan ? Array.from(plan) : [];
        const n = solveQueue.length;
        if (!plan) setStatus("No solution exists from this state.", "error");
        else if (n === 0) setStatus("Already solved.", "solved");
        else setStatus(`Plan: ${plural(n, "move")}. Highlighted cell is next.`);
        paint();
    };

    for (const input of [ui.topology, ui.press, ui.modulus, ui.rows, ui.cols]) {
        input.addEventListener("change", rebuild);
    }
    ui.previewStyle.addEventListener("change", renderPreview);
    ui.scramble.addEventListener("click", scramble);
    ui.solve.addEventListener("click", solve);
    ui.reset.addEventListener("click", reset);
    rebuild();
};

const main = async (): Promise<void> => {
    const roots = document.querySelectorAll(".lights-out");
    try {
        await init();
    } catch (err) {
        for (const root of roots) {
            root.textContent = `Failed to load the Lights Out WebAssembly module: ${errorText(err)}`;
        }
        throw err;
    }
    roots.forEach(mount);
};

main();
