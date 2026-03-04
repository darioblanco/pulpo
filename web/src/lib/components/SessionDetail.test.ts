import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, fireEvent } from '@testing-library/svelte';
import SessionDetail from './SessionDetail.svelte';
import * as api from '$lib/api';
import type { Session, GuardConfig } from '$lib/api';

vi.mock('$lib/api', () => ({
  getSessionOutput: vi.fn(),
  killSession: vi.fn(),
  resumeSession: vi.fn(),
  sendInput: vi.fn(),
  getInterventionEvents: vi.fn(),
}));

// Mock the Terminal component — Svelte 5 components are functions
vi.mock('$lib/components/Terminal.svelte', () => ({
  default: function MockTerminal($$anchor: ChildNode) {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'mock-terminal');
    $$anchor.before(el);
  },
}));

// Mock the ChatView component
vi.mock('$lib/components/ChatView.svelte', () => ({
  default: function MockChatView($$anchor: ChildNode) {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'mock-chat-view');
    el.textContent = 'ChatView';
    $$anchor.before(el);
  },
}));

const mockGetSessionOutput = vi.mocked(api.getSessionOutput);
const mockKillSession = vi.mocked(api.killSession);
const mockResumeSession = vi.mocked(api.resumeSession);
const mockGetInterventionEvents = vi.mocked(api.getInterventionEvents);

afterEach(cleanup);

beforeEach(() => {
  mockGetSessionOutput.mockReset();
  mockKillSession.mockReset();
  mockResumeSession.mockReset();
  mockGetInterventionEvents.mockReset();
});

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'my-api',
    provider: 'claude',
    status: 'running',
    prompt: 'Fix the bug',
    mode: 'interactive',
    workdir: '/home/user/repo',
    guard_config: null,
    model: null,
    allowed_tools: null,
    system_prompt: null,
    metadata: null,
    persona: null,
    intervention_reason: null,
    intervention_at: null,
    max_turns: null,
    max_budget_usd: null,
    output_format: null,
    last_output_at: null,
    waiting_for_input: false,
    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

function makeGuardConfig(preset: string): GuardConfig {
  return { preset };
}

function clickHeader(container: HTMLElement) {
  const header = container.querySelector('[data-testid="session-header"]') as HTMLElement;
  return fireEvent.click(header);
}

describe('SessionDetail', () => {
  it('renders session name, provider, mode, status, and prompt', () => {
    render(SessionDetail, {
      props: { session: makeSession(), onkill: vi.fn() },
    });

    expect(screen.getByText('my-api')).toBeTruthy();
    expect(screen.getByText('claude')).toBeTruthy();
    expect(screen.getByText('interactive')).toBeTruthy();
    expect(screen.getByText('running')).toBeTruthy();
    expect(screen.getByText('Fix the bug')).toBeTruthy();
  });

  it('shows unrestricted guard badge', () => {
    const session = makeSession({ guard_config: makeGuardConfig('unrestricted') });
    render(SessionDetail, {
      props: { session, onkill: vi.fn() },
    });

    expect(screen.getByText('unrestricted')).toBeTruthy();
  });

  it('shows strict guard badge', () => {
    const session = makeSession({ guard_config: makeGuardConfig('strict') });
    render(SessionDetail, {
      props: { session, onkill: vi.fn() },
    });

    expect(screen.getByText('strict')).toBeTruthy();
  });

  it('shows standard guard badge', () => {
    const session = makeSession({ guard_config: makeGuardConfig('standard') });
    render(SessionDetail, {
      props: { session, onkill: vi.fn() },
    });

    expect(screen.getByText('standard')).toBeTruthy();
  });

  it('shows no guard badge when guard_config is null', () => {
    render(SessionDetail, {
      props: { session: makeSession({ guard_config: null }), onkill: vi.fn() },
    });

    expect(screen.queryByText('unrestricted')).toBeNull();
    expect(screen.queryByText('strict')).toBeNull();
    const container = document.querySelector('[data-testid="guard-badge"]');
    expect(container).toBeNull();
  });

  it('toggles expanded state on header click', async () => {
    const { container } = render(SessionDetail, {
      props: { session: makeSession(), onkill: vi.fn() },
    });

    // Initially not expanded — no Kill button visible
    expect(screen.queryByText('Kill Session')).toBeNull();

    await clickHeader(container);

    // Now expanded — Kill button for running session
    expect(screen.getByText('Kill Session')).toBeTruthy();
  });

  it('shows ChatView for expanded non-running session', async () => {
    const session = makeSession({ status: 'completed' });

    const { container } = render(SessionDetail, {
      props: { session, onkill: vi.fn() },
    });

    await clickHeader(container);

    expect(screen.getByTestId('mock-chat-view')).toBeTruthy();
    // No segmented toggle for non-running sessions
    expect(screen.queryByText('Chat')).toBeNull();
    expect(screen.queryByText('Terminal')).toBeNull();
  });

  it('calls killSession and onkill when Kill button clicked', async () => {
    mockKillSession.mockResolvedValue(undefined);
    const onkill = vi.fn();

    const { container } = render(SessionDetail, {
      props: { session: makeSession(), onkill },
    });

    await clickHeader(container);

    const killBtn = screen.getByText('Kill Session');
    await fireEvent.click(killBtn);

    expect(mockKillSession).toHaveBeenCalledWith('sess-1');
    await vi.waitFor(() => {
      expect(onkill).toHaveBeenCalled();
    });
  });

  it('shows Resume button for stale sessions', async () => {
    const session = makeSession({ status: 'stale' });

    const { container } = render(SessionDetail, {
      props: { session, onkill: vi.fn() },
    });

    await clickHeader(container);

    expect(screen.getByText('Resume')).toBeTruthy();
    expect(screen.getByText('Kill Session')).toBeTruthy();
  });

  it('calls resumeSession when Resume clicked', async () => {
    mockResumeSession.mockResolvedValue({ id: 'sess-1', status: 'running' });
    const onkill = vi.fn();
    const session = makeSession({ status: 'stale' });

    const { container } = render(SessionDetail, {
      props: { session, onkill },
    });

    await clickHeader(container);

    const resumeBtn = screen.getByText('Resume');
    await fireEvent.click(resumeBtn);

    expect(mockResumeSession).toHaveBeenCalledWith('sess-1');
    await vi.waitFor(() => {
      expect(onkill).toHaveBeenCalled();
    });
  });

  it('shows Chat/Terminal toggle for running sessions', async () => {
    const { container } = render(SessionDetail, {
      props: { session: makeSession({ status: 'running' }), onkill: vi.fn() },
    });

    await clickHeader(container);

    expect(screen.getByText('Chat')).toBeTruthy();
    expect(screen.getByText('Terminal')).toBeTruthy();
    // Defaults to Chat view
    expect(screen.getByTestId('mock-chat-view')).toBeTruthy();
  });

  it('switches to Terminal view when Terminal button clicked', async () => {
    const { container } = render(SessionDetail, {
      props: { session: makeSession({ status: 'running' }), onkill: vi.fn() },
    });

    await clickHeader(container);

    // Click Terminal tab
    await fireEvent.click(screen.getByText('Terminal'));

    expect(screen.getByTestId('mock-terminal')).toBeTruthy();
  });

  it('collapses expanded section on second header click', async () => {
    const { container } = render(SessionDetail, {
      props: { session: makeSession(), onkill: vi.fn() },
    });

    await clickHeader(container);
    expect(screen.getByText('Kill Session')).toBeTruthy();

    await clickHeader(container);
    expect(screen.queryByText('Kill Session')).toBeNull();
  });

  it('shows intervention badge for dead sessions with intervention_reason', () => {
    const session = makeSession({
      status: 'dead',
      intervention_reason: 'Memory exceeded threshold',
      intervention_at: '2026-01-01T12:00:00Z',
    });
    render(SessionDetail, {
      props: { session, onkill: vi.fn() },
    });

    expect(screen.getByTestId('intervention-badge')).toBeTruthy();
    expect(screen.getByText('intervened')).toBeTruthy();
  });

  it('does not show intervention badge for dead sessions without reason', () => {
    const session = makeSession({ status: 'dead' });
    render(SessionDetail, {
      props: { session, onkill: vi.fn() },
    });

    expect(screen.queryByTestId('intervention-badge')).toBeNull();
  });

  it('shows intervention details when expanded', async () => {
    const session = makeSession({
      status: 'dead',
      intervention_reason: 'Memory exceeded threshold',
      intervention_at: '2026-01-01T12:00:00Z',
    });
    mockGetInterventionEvents.mockResolvedValue([]);

    const { container } = render(SessionDetail, {
      props: { session, onkill: vi.fn() },
    });

    await clickHeader(container);

    expect(screen.getByText(/Memory exceeded threshold/)).toBeTruthy();
    expect(screen.getByTestId('interventions-toggle')).toBeTruthy();
    expect(screen.getByText('Show history')).toBeTruthy();
  });

  it('loads and shows intervention history when toggle clicked', async () => {
    const session = makeSession({
      status: 'dead',
      intervention_reason: 'Memory exceeded threshold',
      intervention_at: '2026-01-01T12:00:00Z',
    });
    mockGetInterventionEvents.mockResolvedValue([
      { id: 1, session_id: 'sess-1', reason: 'OOM kill', created_at: '2026-01-01T12:00:00Z' },
    ]);

    const { container } = render(SessionDetail, {
      props: { session, onkill: vi.fn() },
    });

    await clickHeader(container);

    const toggle = screen.getByTestId('interventions-toggle');
    await fireEvent.click(toggle);

    await vi.waitFor(() => {
      expect(mockGetInterventionEvents).toHaveBeenCalledWith('sess-1');
    });

    await vi.waitFor(() => {
      expect(screen.getByTestId('intervention-history')).toBeTruthy();
      expect(screen.getByText('OOM kill')).toBeTruthy();
      expect(screen.getByText('Hide history')).toBeTruthy();
    });
  });
});
