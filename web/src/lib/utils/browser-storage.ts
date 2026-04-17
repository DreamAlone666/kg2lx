const PREFIX = 'kg2lx:';

export function setLocal(key: string, value: string) {
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(PREFIX + key, value);
  }
}

export function getLocal(key: string): string | null {
  if (typeof localStorage !== 'undefined') {
    return localStorage.getItem(PREFIX + key);
  }
  return null;
}

export function setSession(key: string, value: string) {
  if (typeof sessionStorage !== 'undefined') {
    sessionStorage.setItem(PREFIX + key, value);
  }
}

export function getSession(key: string): string | null {
  if (typeof sessionStorage !== 'undefined') {
    return sessionStorage.getItem(PREFIX + key);
  }
  return null;
}

export function clearSession(key: string) {
  if (typeof sessionStorage !== 'undefined') {
    sessionStorage.removeItem(PREFIX + key);
  }
}
