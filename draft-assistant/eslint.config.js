import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    // Generated output: the Vite build, the v8 coverage report (ships .js
    // files with stale eslint-disable directives), Playwright's reports and
    // the dogfood artefacts. Linting any of them fails `--max-warnings=0`.
    ignores: [
      "dist",
      "coverage",
      "playwright-report",
      "test-results",
      "src-tauri/target",
      "dogfood-output",
    ],
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    extends: [
      js.configs.recommended,
      ...tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
    ],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    plugins: {
      "react-refresh": reactRefresh,
    },
    rules: {
      "react-refresh/only-export-components": [
        "error",
        { allowConstantExport: true },
      ],
    },
  },
);
