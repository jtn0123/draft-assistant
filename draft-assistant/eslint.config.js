import js from "@eslint/js";
import globals from "globals";
import jsxA11y from "eslint-plugin-jsx-a11y";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["coverage", "dist", "src-tauri/target"] },
  // The build and lint configs themselves, plus the guard scripts. Previously
  // nothing checked these at all: they sat outside both the ESLint glob and
  // the typechecked project.
  {
    files: ["scripts/**/*.mjs", "*.config.{ts,js}", "eslint.config.js"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.node,
    },
  },
  // The browser end-to-end suite. Node-side test code, not app code: no React
  // rules, and not in the app's typechecked project either — Playwright brings
  // its own types.
  {
    files: ["e2e-browser/**/*.ts"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: {
      ecmaVersion: 2022,
      globals: { ...globals.node, ...globals.browser },
    },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    extends: [
      js.configs.recommended,
      // Type-aware, not just syntactic. Everything this app does with the
      // backend is an async IPC call, so no-floating-promises and
      // no-misused-promises are the rules that matter most here; the
      // non-checked preset cannot see either.
      ...tseslint.configs.recommendedTypeChecked,
      reactHooks.configs.flat.recommended,
      // Accessibility, on the strict preset. The app is a keyboard-heavy draft
      // board: every defect this catches (a click handler with no keyboard
      // path, a dialog with no label, a fact hidden in a hover-only tooltip)
      // is one a real user would hit. --max-warnings=0 makes it a gate.
      jsxA11y.flatConfigs.strict,
    ],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      "react-refresh": reactRefresh,
    },
    rules: {
      "react-refresh/only-export-components": ["error", { allowConstantExport: true }],
    },
  },
);
