import { describe, it, expect } from 'vitest';
import { getProviderCapabilities, getProviderModels } from './types';

describe('getProviderCapabilities', () => {
  it('returns full capabilities for claude', () => {
    const caps = getProviderCapabilities('claude');
    expect(caps.model).toBe(true);
    expect(caps.system_prompt).toBe(true);
    expect(caps.guard_preset).toBe(true);
  });

  it('returns limited capabilities for codex', () => {
    const caps = getProviderCapabilities('codex');
    expect(caps.model).toBe(true);
    expect(caps.system_prompt).toBe(false);
    expect(caps.guard_preset).toBe(false);
  });

  it('returns model support for gemini', () => {
    const caps = getProviderCapabilities('gemini');
    expect(caps.model).toBe(true);
    expect(caps.guard_preset).toBe(true);
  });

  it('returns no model support for open_code', () => {
    const caps = getProviderCapabilities('open_code');
    expect(caps.model).toBe(false);
  });

  it('returns no model support for opencode alias', () => {
    const caps = getProviderCapabilities('opencode');
    expect(caps.model).toBe(false);
  });

  it('returns full capabilities for unknown provider', () => {
    const caps = getProviderCapabilities('unknown');
    expect(caps.model).toBe(true);
    expect(caps.system_prompt).toBe(true);
  });
});

describe('getProviderModels', () => {
  it('returns Claude model shortcuts', () => {
    const models = getProviderModels('claude');
    expect(models).toEqual([
      { value: 'opus', label: 'Opus' },
      { value: 'sonnet', label: 'Sonnet' },
      { value: 'haiku', label: 'Haiku' },
    ]);
  });

  it('returns Codex model shortcuts', () => {
    const models = getProviderModels('codex');
    expect(models).toEqual([
      { value: 'o3', label: 'o3' },
      { value: 'o4-mini', label: 'o4-mini' },
    ]);
  });

  it('returns Gemini model shortcuts', () => {
    const models = getProviderModels('gemini');
    expect(models).toEqual([
      { value: 'gemini-2.5-pro', label: '2.5 Pro' },
      { value: 'gemini-2.5-flash', label: '2.5 Flash' },
    ]);
  });

  it('returns empty array for open_code', () => {
    expect(getProviderModels('open_code')).toEqual([]);
  });

  it('returns empty array for unknown provider', () => {
    expect(getProviderModels('unknown')).toEqual([]);
  });
});
