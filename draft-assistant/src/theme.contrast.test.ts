// The stylesheets themselves, read as text.
//
// Colour and focus are decided in CSS, so this is the only place they can be
// checked. jsdom applies no stylesheet and computes no contrast, and a real
// browser run (`e2e-browser/`) would not tell us *why* a shade failed. So the
// tokens are parsed out of the sheets and the arithmetic done here: WCAG AA
// asks for 4.5:1 on body-sized text, and every one of these is body-sized.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/** Vitest runs from the package root, and the sheets all live one level in. */
const sheet = (name: string) => readFileSync(resolve(process.cwd(), "src", name), "utf8");

const theme = sheet("theme.css");
const board = sheet("board.css");

/** The tokens declared in one `:root`-ish block of theme.css. */
function tokens(block: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [, name, value] of block.matchAll(/(--[\w-]+):\s*([^;]+);/g)) {
    out[name] = value.trim();
  }
  return out;
}

function blockAfter(selector: string): string {
  const at = theme.indexOf(selector);
  expect(at).toBeGreaterThanOrEqual(0);
  const open = theme.indexOf("{", at);
  return theme.slice(open, theme.indexOf("}", open));
}

const light = tokens(blockAfter(":root {"));
const dark = tokens(blockAfter('[data-theme="dark"]'));

function rgb(hex: string): [number, number, number] {
  const h = hex.trim().replace("#", "");
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)) as [number, number, number];
}

function luminance(hex: string): number {
  const [r, g, b] = rgb(hex).map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG's contrast ratio, 1:1 (identical) to 21:1 (black on white). */
function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** `color-mix(in srgb, <tint> 18%, transparent)` over an opaque ground, which
 *  is what the board's imported-rank cells actually paint. */
function tinted(tint: string, ground: string, share = 0.18): string {
  const [a, b] = [rgb(tint), rgb(ground)];
  const mixed = a.map((c, i) => Math.round(c * share + b[i] * (1 - share)));
  return `#${mixed.map((c) => c.toString(16).padStart(2, "0")).join("")}`;
}

const AA = 4.5;

describe("the faint text token", () => {
  it("reads against the grounds it is painted on, in both themes", () => {
    for (const [name, set] of [
      ["light", light],
      ["dark", dark],
    ] as const) {
      for (const ground of ["--panel", "--paper"] as const) {
        expect(
          contrast(set["--faint"], set[ground]),
          `--faint on ${ground} (${name})`,
        ).toBeGreaterThanOrEqual(AA);
      }
    }
  });

  it("still sits below the middle tone, so the hierarchy survives the fix", () => {
    // Faint is meant to recede. Legible is not the same as equal billing.
    for (const set of [light, dark]) {
      expect(contrast(set["--faint"], set["--panel"])).toBeLessThan(
        contrast(set["--mid"], set["--panel"]),
      );
    }
  });
});

describe("the imported-rank cells on the board", () => {
  it("puts readable type on its own tint", () => {
    expect(board).toContain("color: var(--posink)");
    expect(board).toContain("color: var(--actink)");
    for (const set of [light, dark]) {
      expect(
        contrast(set["--posink"], tinted(set["--pos"], set["--panel"])),
      ).toBeGreaterThanOrEqual(AA);
      expect(
        contrast(set["--actink"], tinted(set["--act"], set["--panel"])),
      ).toBeGreaterThanOrEqual(AA);
    }
  });
});

describe("what the stylesheets are allowed to name", () => {
  it("uses no colour token the theme does not declare", () => {
    const declared = new Set([...Object.keys(light), ...Object.keys(dark)]);
    for (const name of ["App", "header", "board", "components", "bits", "chat", "season"]) {
      for (const [, used] of sheet(`${name}.css`).matchAll(/var\((--[\w-]+)/g)) {
        expect(declared.has(used), `${name}.css uses ${used}`).toBe(true);
      }
    }
  });

  it("gives every focusable control the same ring", () => {
    // A select, a textarea and a link take focus like a button does, and used
    // to take it invisibly.
    const ring = theme.slice(theme.indexOf("button:focus-visible"));
    for (const selector of ["select", "textarea", "a", "input", "[tabindex]"]) {
      expect(ring.slice(0, ring.indexOf("{"))).toContain(`${selector}:focus-visible`);
    }
  });
});
