import type { ApiErrorPayload } from './types';

export class AdminApiError extends Error {
  code?: string;
  status: number;

  constructor(status: number, payload: ApiErrorPayload) {
    super(payload.message || `API error ${status}`);
    this.name = 'AdminApiError';
    this.status = status;
    this.code = payload.code;
  }
}

export function isAdminApiError(error: unknown): error is AdminApiError {
  return error instanceof AdminApiError;
}
