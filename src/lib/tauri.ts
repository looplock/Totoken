import { invoke, isTauri } from '@tauri-apps/api/core';

type AppErrorPayload = {
  code?: unknown;
  message?: unknown;
};

export class InvokeCommandError extends Error {
  code?: string;
  command: string;
  details?: unknown;

  constructor({
    command,
    message,
    code,
    details,
  }: {
    command: string;
    message: string;
    code?: string;
    details?: unknown;
  }) {
    super(message);
    this.name = 'InvokeCommandError';
    this.command = command;
    this.code = code;
    this.details = details;
  }
}

export function isTauriRuntime(): boolean {
  return isTauri();
}

export async function invokeCommand<T>(command: string, args?: Record<string, unknown>) {
  if (!isTauriRuntime()) {
    throw new Error('Tauri runtime is unavailable.');
  }

  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeInvokeError(command, error);
  }
}

function normalizeInvokeError(command: string, error: unknown): Error {
  if (error instanceof Error) {
    return error;
  }

  const payload = extractAppErrorPayload(error);
  if (payload) {
    return new InvokeCommandError({
      command,
      message: payload.message,
      code: payload.code,
      details: error,
    });
  }

  if (typeof error === 'string' && error.trim().length > 0) {
    return new InvokeCommandError({
      command,
      message: error,
      details: error,
    });
  }

  const fallbackMessage = buildFallbackErrorMessage(command, error);
  return new InvokeCommandError({
    command,
    message: fallbackMessage,
    details: error,
  });
}

function extractAppErrorPayload(error: unknown): { code?: string; message: string } | null {
  if (!isRecord(error)) {
    return null;
  }

  const { code, message } = error as AppErrorPayload;
  if (typeof message !== 'string' || message.trim().length === 0) {
    return null;
  }

  return {
    message,
    code: typeof code === 'string' && code.trim().length > 0 ? code : undefined,
  };
}

function buildFallbackErrorMessage(command: string, error: unknown): string {
  const serialized = safeSerialize(error);
  if (serialized) {
    return serialized;
  }

  return `Command "${command}" failed.`;
}

function safeSerialize(error: unknown): string | null {
  if (error === null || error === undefined) {
    return null;
  }

  if (typeof error === 'number' || typeof error === 'boolean' || typeof error === 'bigint') {
    return String(error);
  }

  if (typeof error === 'object') {
    try {
      return JSON.stringify(error);
    } catch {
      return null;
    }
  }

  return null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}
