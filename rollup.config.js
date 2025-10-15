import url from "@rollup/plugin-url";
import typescript from "@rollup/plugin-typescript";

export default {
  input: "ts/binary.ts",
  output: {
    dir: "dist",
    format: "esm",
    entryFileNames: "binary.js",
  },
  plugins: [
    typescript({
      declaration: false,
    }),
    url({
      include: ["**/*.wasm"],
      limit: Infinity,
    }),
  ],
};
