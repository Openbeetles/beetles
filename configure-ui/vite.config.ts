import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const host = process.env.TAURI_DEV_HOST
const tauriTarget = process.env.TAURI_ENV_PLATFORM
  ? process.env.TAURI_ENV_PLATFORM === 'windows'
    ? 'chrome105'
    : 'safari13'
  : undefined

export default defineConfig({
  clearScreen: false,
  plugins: [react()],
  base: process.env.VITE_BASE_PATH ?? '/',
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    target: tauriTarget,
    minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    /**
     * 技能富编辑器已拆成独立懒加载链；将警戒线校准到该受控 vendor 块之上，
     * 继续拦截更大的异常回归，同时避免 500 kB 默认阈值对按需编辑器资源误报。
     */
    chunkSizeWarningLimit: 1200,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (
            id.includes('node_modules/react') ||
            id.includes('node_modules/react-dom') ||
            id.includes('node_modules/scheduler') ||
            id.includes('node_modules/react-router')
          ) {
            return 'react-vendor'
          }
          if (id.includes('node_modules/i18next') || id.includes('node_modules/react-i18next')) {
            return 'i18n-vendor'
          }
          if (
            id.includes('node_modules/@mdxeditor/') ||
            id.includes('node_modules/lexical') ||
            id.includes('node_modules/@lexical/') ||
            id.includes('node_modules/prosemirror') ||
            id.includes('node_modules/yjs')
          ) {
            return 'skill-editor-vendor'
          }
          if (
            id.includes('node_modules/mdast') ||
            id.includes('node_modules/micromark') ||
            id.includes('node_modules/remark') ||
            id.includes('node_modules/rehype') ||
            id.includes('node_modules/unified') ||
            id.includes('node_modules/unist') ||
            id.includes('node_modules/hast') ||
            id.includes('node_modules/vfile')
          ) {
            return 'skill-editor-markdown'
          }
        },
      },
    },
  },
})
