const PREFIX = 'kg2lx:';

function useStorage<T>(
  type: 'localStorage' | 'sessionStorage',
  action: (storage: Storage) => T,
  fallback: T
): T {
  try {
    const storage = globalThis[type];
    return storage ? action(storage) : fallback;
  } catch {
    return fallback;
  }
}

export function setLocal(key: string, value: string) {
  useStorage('localStorage', storage => storage.setItem(PREFIX + key, value), undefined);
}

export function getLocal(key: string): string | null {
  return useStorage('localStorage', storage => storage.getItem(PREFIX + key), null);
}

export function setSession(key: string, value: string) {
  useStorage('sessionStorage', storage => storage.setItem(PREFIX + key, value), undefined);
}

export function getSession(key: string): string | null {
  return useStorage('sessionStorage', storage => storage.getItem(PREFIX + key), null);
}

export function clearSession(key: string) {
  useStorage('sessionStorage', storage => storage.removeItem(PREFIX + key), undefined);
}
