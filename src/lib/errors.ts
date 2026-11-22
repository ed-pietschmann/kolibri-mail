import { ApiError } from './api';
import { t, type MessageKey } from './i18n';

/** Verständliche deutsche Meldung für einen Backend-Fehler. */
export function describeError(err: unknown): string {
  if (!(err instanceof ApiError)) return t('error.internal');
  const server = err.server ? t(`error.server.${err.server}` as MessageKey) : '';
  const text = t(`error.${err.kind}` as MessageKey, { server });
  // Doppelte Leerzeichen entfernen, falls kein Server bekannt ist.
  return text.replace(/\s{2,}/g, ' ');
}
