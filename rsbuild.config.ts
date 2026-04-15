import { defineConfig } from "@rsbuild/core";
import { pluginBabel } from "@rsbuild/plugin-babel";
import { pluginSolid } from "@rsbuild/plugin-solid";
import tailwindcss from "@tailwindcss/postcss";

export default defineConfig({
  plugins: [
    pluginBabel({
      include: /\.(?:jsx|tsx)$/,
    }),
    pluginSolid(),
  ],
  source: {
    entry: {
      index: "./src/index.tsx",
    },
  },
  html: {
    template: "./index.html",
  },
  output: {
    distPath: {
      root: "dist",
    },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
  tools: {
    postcss: {
      postcssOptions: {
        plugins: [tailwindcss()],
      },
    },
  },
});
