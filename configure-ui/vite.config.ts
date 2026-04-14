import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
// GitHub Pages 部署时通过 VITE_BASE_PATH 设置 base（如 /beetle/），本地开发默认 /
export default defineConfig({
  plugins: [react()],
  base: process.env.VITE_BASE_PATH ?? '/',
  build: {
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
