import js from '@eslint/js';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';

/** @type {import('eslint').Linter.Config[]} */
export default [
  js.configs.recommended,
  ...svelte.configs['flat/recommended'],
  {
    languageOptions: {
      globals: { ...globals.browser, ...globals.webextensions },
    },
  },
  {
    files: ['**/*.svelte'],
    languageOptions: {
      parserOptions: {
        parser: svelte.parser,
        extraFileExtensions: ['.svelte'],
      },
    },
  },
  {
    ignores: ['dist/', 'node_modules/', 'src-tauri/', 'src-host/', 'scripts/', '*.config.js'],
  },
];
