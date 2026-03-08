import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { InkSettings } from './ink-settings';
import * as api from '@/api/client';
import type { InkConfig, PeerInfo, UpdateConfigResponse } from '@/api/types';

vi.mock('@/api/client', () => ({
  updateRemoteConfig: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

const mockUpdateRemoteConfig = vi.mocked(api.updateRemoteConfig);

const emptyInk: InkConfig = {
  description: null,
  provider: null,
  model: null,
  mode: null,
  guard_preset: null,
  instructions: null,
};

const reviewerInk: InkConfig = {
  description: 'Code review specialist',
  provider: 'claude',
  model: null,
  mode: 'interactive',
  guard_preset: 'strict',
  instructions: 'You are a code reviewer.',
};

const onlinePeer: PeerInfo = {
  name: 'remote-node',
  address: 'remote:7433',
  status: 'online',
  node_info: null,
  session_count: null,
};

const offlinePeer: PeerInfo = {
  name: 'offline-node',
  address: 'offline:7433',
  status: 'offline',
  node_info: null,
  session_count: null,
};

beforeEach(() => {
  mockUpdateRemoteConfig.mockReset();
});

describe('InkSettings', () => {
  it('renders the card', () => {
    render(<InkSettings inks={{}} onInksChange={vi.fn()} />);
    expect(screen.getByTestId('ink-settings')).toBeInTheDocument();
    expect(screen.getByText('Inks')).toBeInTheDocument();
  });

  it('shows empty state when no inks', () => {
    render(<InkSettings inks={{}} onInksChange={vi.fn()} />);
    expect(screen.getByTestId('ink-empty')).toBeInTheDocument();
  });

  it('lists ink names sorted alphabetically', () => {
    const inks = {
      coder: { ...emptyInk },
      reviewer: { ...reviewerInk },
      'quick-fix': { ...emptyInk },
    };
    render(<InkSettings inks={inks} onInksChange={vi.fn()} />);
    expect(screen.getByTestId('ink-coder')).toBeInTheDocument();
    expect(screen.getByTestId('ink-quick-fix')).toBeInTheDocument();
    expect(screen.getByTestId('ink-reviewer')).toBeInTheDocument();
  });

  it('shows ink description in collapsed view', () => {
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} />);
    expect(screen.getByText('Code review specialist')).toBeInTheDocument();
  });

  it('expands ink editor on click', () => {
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} />);
    expect(screen.queryByTestId('ink-editor-reviewer')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    expect(screen.getByTestId('ink-editor-reviewer')).toBeInTheDocument();
  });

  it('collapses ink editor on second click', () => {
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    expect(screen.getByTestId('ink-editor-reviewer')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    expect(screen.queryByTestId('ink-editor-reviewer')).not.toBeInTheDocument();
  });

  it('displays ink fields in editor', () => {
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    expect(screen.getByLabelText('Description')).toHaveValue('Code review specialist');
    expect(screen.getByLabelText('Instructions')).toHaveValue('You are a code reviewer.');
  });

  it('calls onInksChange when description is updated', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('Description'), {
      target: { value: 'Updated desc' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, description: 'Updated desc' },
    });
  });

  it('calls onInksChange when instructions is updated', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('Instructions'), {
      target: { value: 'New prompt' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, instructions: 'New prompt' },
    });
  });

  it('clears field to null when emptied', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('Description'), {
      target: { value: '' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, description: null },
    });
  });

  it('updates model field', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('Model'), {
      target: { value: 'opus' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, model: 'opus' },
    });
  });

  it('removes an ink', () => {
    const onInksChange = vi.fn();
    const inks = { reviewer: reviewerInk, coder: { ...emptyInk } };
    render(<InkSettings inks={inks} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.click(screen.getByTestId('ink-remove-reviewer'));
    expect(onInksChange).toHaveBeenCalledWith({ coder: { ...emptyInk } });
  });

  it('adds a new ink', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{}} onInksChange={onInksChange} />);
    fireEvent.change(screen.getByTestId('ink-new-name'), {
      target: { value: 'my-ink' },
    });
    fireEvent.click(screen.getByTestId('ink-add-btn'));
    expect(onInksChange).toHaveBeenCalledWith({
      'my-ink': { ...emptyInk },
    });
  });

  it('normalizes new ink name to kebab-case', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{}} onInksChange={onInksChange} />);
    fireEvent.change(screen.getByTestId('ink-new-name'), {
      target: { value: 'My Custom Ink' },
    });
    fireEvent.click(screen.getByTestId('ink-add-btn'));
    expect(onInksChange).toHaveBeenCalledWith({
      'my-custom-ink': { ...emptyInk },
    });
  });

  it('prevents adding ink with duplicate name', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.change(screen.getByTestId('ink-new-name'), {
      target: { value: 'reviewer' },
    });
    fireEvent.click(screen.getByTestId('ink-add-btn'));
    expect(onInksChange).not.toHaveBeenCalled();
  });

  it('add button is disabled when name is empty', () => {
    render(<InkSettings inks={{}} onInksChange={vi.fn()} />);
    expect(screen.getByTestId('ink-add-btn')).toBeDisabled();
  });

  it('adds ink on Enter key', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{}} onInksChange={onInksChange} />);
    const input = screen.getByTestId('ink-new-name');
    fireEvent.change(input, { target: { value: 'test-ink' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onInksChange).toHaveBeenCalledWith({
      'test-ink': { ...emptyInk },
    });
  });

  it('does not show empty state when inks exist', () => {
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} />);
    expect(screen.queryByTestId('ink-empty')).not.toBeInTheDocument();
  });

  // Phase B: Push to peers
  it('does not show push button when no peers', () => {
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} />);
    expect(screen.queryByTestId('ink-push-btn')).not.toBeInTheDocument();
  });

  it('does not show push button when no online peers', () => {
    render(
      <InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} peers={[offlinePeer]} />,
    );
    expect(screen.queryByTestId('ink-push-btn')).not.toBeInTheDocument();
  });

  it('does not show push button when no inks', () => {
    render(<InkSettings inks={{}} onInksChange={vi.fn()} peers={[onlinePeer]} />);
    expect(screen.queryByTestId('ink-push-btn')).not.toBeInTheDocument();
  });

  it('shows push button when online peers and inks exist', () => {
    render(
      <InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} peers={[onlinePeer]} />,
    );
    expect(screen.getByTestId('ink-push-btn')).toBeInTheDocument();
    expect(screen.getByText(/1 online peer/)).toBeInTheDocument();
  });

  it('shows plural peers text for multiple online peers', () => {
    const peer2: PeerInfo = { ...onlinePeer, name: 'peer-2', address: 'peer2:7433' };
    render(
      <InkSettings
        inks={{ reviewer: reviewerInk }}
        onInksChange={vi.fn()}
        peers={[onlinePeer, peer2, offlinePeer]}
      />,
    );
    expect(screen.getByText(/2 online peers/)).toBeInTheDocument();
  });

  it('pushes inks to all online peers on click', async () => {
    mockUpdateRemoteConfig.mockResolvedValue({
      config: {} as UpdateConfigResponse['config'],
      restart_required: false,
    });
    render(
      <InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} peers={[onlinePeer]} />,
    );

    fireEvent.click(screen.getByTestId('ink-push-btn'));

    await waitFor(() => {
      expect(mockUpdateRemoteConfig).toHaveBeenCalledWith('remote:7433', {
        inks: { reviewer: reviewerInk },
      });
    });

    await waitFor(() => {
      expect(screen.getByTestId('ink-push-result')).toHaveTextContent('remote-node: ok');
    });
  });

  it('shows per-peer error on push failure', async () => {
    mockUpdateRemoteConfig.mockRejectedValue(new Error('unauthorized'));
    render(
      <InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} peers={[onlinePeer]} />,
    );

    fireEvent.click(screen.getByTestId('ink-push-btn'));

    await waitFor(() => {
      expect(screen.getByTestId('ink-push-result')).toHaveTextContent('remote-node: unauthorized');
    });
  });

  it('pushes to multiple peers with mixed results', async () => {
    const peer2: PeerInfo = { ...onlinePeer, name: 'peer-2', address: 'peer2:7433' };
    mockUpdateRemoteConfig
      .mockResolvedValueOnce({
        config: {} as UpdateConfigResponse['config'],
        restart_required: false,
      })
      .mockRejectedValueOnce(new Error('timeout'));

    render(
      <InkSettings
        inks={{ reviewer: reviewerInk }}
        onInksChange={vi.fn()}
        peers={[onlinePeer, peer2]}
      />,
    );

    fireEvent.click(screen.getByTestId('ink-push-btn'));

    await waitFor(() => {
      const result = screen.getByTestId('ink-push-result');
      expect(result).toHaveTextContent('remote-node: ok');
      expect(result).toHaveTextContent('peer-2: timeout');
    });
  });
});
