// throwaway: production-build the 3D harness with the same pipeline
import baseConfigFn from "./vite.config"
import { mergeConfig, type UserConfig } from "vite"

export default async (env: never) => {
  const base = await (baseConfigFn as unknown as (e: never) => Promise<UserConfig>)(env)
  return mergeConfig(base, {
    build: {
      outDir: "/tmp/vt-dist",
      emptyOutDir: true,
      rolldownOptions: { input: { test3d: "test-3d.html" } },
    },
  })
}
