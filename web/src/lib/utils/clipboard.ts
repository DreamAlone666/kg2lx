/**
 * 剪贴板写入工具。
 *
 * Clipboard API 仅在 HTTPS 或 localhost 下可用；
 * 本工具常见纯 HTTP 内网部署，此时必须降级到 execCommand，
 * 否则复制会静默失败而用户毫不知情。
 */
export async function writeToClipboard(text: string): Promise<boolean> {
	try {
		await navigator.clipboard.writeText(text);
		return true;
	} catch {
		return legacyCopy(text);
	}
}

function legacyCopy(text: string): boolean {
	if (typeof document === 'undefined') return false;
	const textarea = document.createElement('textarea');
	textarea.value = text;
	textarea.setAttribute('readonly', '');
	textarea.style.position = 'fixed';
	textarea.style.opacity = '0';
	document.body.appendChild(textarea);
	textarea.select();
	let ok = false;
	try {
		ok = document.execCommand('copy');
	} catch {
		ok = false;
	}
	textarea.remove();
	return ok;
}
