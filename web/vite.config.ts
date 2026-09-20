import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { loadEnv } from 'vite';
import { defineConfig } from 'vite';

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, '.', '');
  const backendTarget = env.VITE_BACKEND_TARGET || 'http://127.0.0.1:8787';

  return {
    plugins: [tailwindcss(), sveltekit()],
    server: {
      // 原样转发浏览器实际调用的那组后端路径。开发期没有独立的「后端前缀」——
      // 连接页留空即当前域名，与生产同源部署的口径一致，故这里不能改写路径。
      // 前缀取自前端自己的调用面（GET /healthz 与 /api/v1/admin/**）；前端若新增这个
      // 前缀之外的调用，这里要跟着加，否则开发期会落到 SPA 的 HTML 回退上。
      proxy: {
        '/healthz': backendTarget,
        '/api': backendTarget,
      },
    },
  };
});
