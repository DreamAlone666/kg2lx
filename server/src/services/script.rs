use std::sync::Arc;

use crate::config::Config;
use crate::error::AppError;
use crate::repos::source::SourceRepo;

pub struct ScriptService {
    config: Arc<Config>,
    source_repo: Arc<dyn SourceRepo>,
}

impl ScriptService {
    pub fn new(config: Arc<Config>, source_repo: Arc<dyn SourceRepo>) -> Self {
        Self {
            config,
            source_repo,
        }
    }

    pub async fn render_script_by_token(
        &self,
        script_token: &str,
    ) -> Result<(axum::http::HeaderMap, String), AppError> {
        let source = self
            .source_repo
            .find_by_script_token(script_token)
            .await?
            .ok_or_else(AppError::source_not_found)?;

        if !source.enabled {
            return Err(AppError::source_disabled());
        }

        let public_base_url = &self.config.public_base_url;
        let runtime_token = &source.runtime_token;
        let source_name = &self.config.source_name_prefix;

        let script = format!(
            r#"/**
 * @name {source_name}
 * @description Per-account Kugou Concept VIP source
 * @version 0.1.0
 */
const {{ EVENT_NAMES, request, on, send }} = globalThis.lx
const API = '{public_base_url}/api/v1/runtime/music-url'
const TOKEN = '{runtime_token}'
const qualityMap = {{ '128k': '128k', '320k': '320k', flac: 'flac', flac24bit: 'flac24bit' }}
const httpPost = (url, body) => new Promise((resolve, reject) => {{
  request(url, {{
    method: 'POST',
    headers: {{
      'content-type': 'application/json',
      authorization: `Bearer ${{TOKEN}}`,
    }},
    body: JSON.stringify(body),
  }}, (err, resp) => {{
    if (err) return reject(err)
    try {{
      const data = typeof resp.body === 'string' ? JSON.parse(resp.body) : (resp.body || {{}})
      if (resp.statusCode && resp.statusCode >= 400) return reject(new Error(data.message || `request failed: ${{resp.statusCode}}`))
      if (!data.url) return reject(new Error('empty url'))
      resolve(data.url)
    }} catch (e) {{
      reject(e)
    }}
  }})
}})
on(EVENT_NAMES.request, ({{ source, action, info }}) => {{
  if (source !== 'kg' || action !== 'musicUrl') return Promise.reject(new Error('unsupported action'))
  const musicInfo = info.musicInfo || {{}}
  const quality = qualityMap[info.type]
  if (!quality) return Promise.reject(new Error(`unsupported quality: ${{info.type}}`))
  return httpPost(API, {{
    hash: musicInfo.hash,
    album_audio_id: musicInfo.album_audio_id || musicInfo.albumAudioId || null,
    quality,
  }})
}})
send(EVENT_NAMES.inited, {{
  openDevTools: false,
  sources: {{
    kg: {{
      name: '{source_name}',
      type: 'music',
      actions: ['musicUrl'],
      qualitys: ['128k', '320k', 'flac', 'flac24bit'],
    }},
  }},
}})
"#
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            "application/javascript; charset=utf-8".parse().unwrap(),
        );
        headers.insert(
            axum::http::header::CACHE_CONTROL,
            "no-store".parse().unwrap(),
        );

        Ok((headers, script))
    }
}
