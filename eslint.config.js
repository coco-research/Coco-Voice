import i18next from "eslint-plugin-i18next";
import tsParser from "@typescript-eslint/parser";

export default [
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaFeatures: {
          jsx: true,
        },
      },
    },
    plugins: {
      i18next,
    },
    rules: {
      "i18next/no-literal-string": [
        "error",
        {
          mode: "jsx-only",
          "jsx-attributes": {
            include: [
              "title",
              "placeholder",
              "aria-label",
              "ariaLabel",
              "label",
              "description",
              "alt",
            ],
          },
          callees: {
            exclude: ["i18n(ext)?", "t", "isUpdating", "renderSettingsContent"],
          },
          "object-properties": {
            exclude: [".*"],
          },
        },
      ],
    },
  },
];
