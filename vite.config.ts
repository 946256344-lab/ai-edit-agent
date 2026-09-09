// Vite 前端构建入口：装配 React 插件和桌面开发服务器参数，不承载业务逻辑。
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  // Windows：勿监视 Cargo 输出，否则 beforeDevCommand 会因 EBUSY 锁定 DLL 退出。
  server: {
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
})
