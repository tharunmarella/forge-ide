import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { viteSingleFile } from 'vite-plugin-singlefile'
import path from 'path'

export default defineConfig({
  plugins: [react(), tailwindcss(), viteSingleFile()],
  resolve: {
    alias: { '@': path.resolve(__dirname, './src') },
  },
  base: './',
  build: {
    // Single self-contained index.html — easiest to extract from classpath in Kotlin
    outDir: '../src/main/resources/webview/dist',
    emptyOutDir: true,
    cssCodeSplit: false,
  },
})
