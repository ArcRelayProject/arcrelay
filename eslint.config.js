import tseslint from "typescript-eslint";
import svelte from "eslint-plugin-svelte";

export default [
  { ignores: ["**/node_modules/**", "**/dist/**", "frontend/src/ipc/generated.ts"] },
  ...svelte.configs["flat/base"],
  {
    files: ["frontend/src/**/*.ts", "frontend/src/**/*.svelte"],
    plugins: { "@typescript-eslint": tseslint.plugin },
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: {
        project: "./frontend/tsconfig.json",
        tsconfigRootDir: import.meta.dirname,
        extraFileExtensions: [".svelte"],
      },
    },
    rules: {
      eqeqeq: ["error", "always", { null: "ignore" }],
      "@typescript-eslint/no-floating-promises": "error",
      "@typescript-eslint/no-misused-promises": ["error", { checksVoidReturn: false }],
    },
  },
  {
    files: ["frontend/src/**/*.test.ts"],
    rules: {
      "@typescript-eslint/no-floating-promises": [
        "error",
        { allowForKnownSafeCalls: [{ from: "package", name: "test", package: "node:test" }] },
      ],
    },
  },
  {
    files: ["frontend/src/**/*.svelte"],
    languageOptions: {
      parser: svelte.configs["flat/base"].find((config) => config.languageOptions?.parser)
        ?.languageOptions.parser,
      parserOptions: {
        parser: tseslint.parser,
        project: "./frontend/tsconfig.json",
        tsconfigRootDir: import.meta.dirname,
        extraFileExtensions: [".svelte"],
      },
    },
  },
];
