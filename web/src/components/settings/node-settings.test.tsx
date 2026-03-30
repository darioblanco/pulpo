import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { NodeSettings } from './node-settings';

const defaults = {
  name: 'my-node',
  onNameChange: vi.fn(),
  port: 7433,
  onPortChange: vi.fn(),
  dataDir: '/data',
  onDataDirChange: vi.fn(),
  bind: 'local',
  onBindChange: vi.fn(),
  tag: '',
  onTagChange: vi.fn(),
  discoveryInterval: 60,
  onDiscoveryIntervalChange: vi.fn(),
};

describe('NodeSettings', () => {
  it('renders core fields', () => {
    render(<NodeSettings {...defaults} />);
    expect(screen.getByTestId('node-settings')).toBeInTheDocument();
    expect(screen.getByLabelText('Name')).toHaveValue('my-node');
    expect(screen.getByLabelText('Port')).toHaveValue(7433);
    expect(screen.getByLabelText('Data directory')).toHaveValue('/data');
    expect(screen.getByLabelText('Tag')).toHaveValue('');
  });

  it('renders bind mode select', () => {
    render(<NodeSettings {...defaults} />);
    expect(screen.getByTestId('bind-mode-trigger')).toBeInTheDocument();
  });

  it('hides networking fields in local mode', () => {
    render(<NodeSettings {...defaults} bind="local" />);
    expect(screen.queryByLabelText('Discovery interval')).not.toBeInTheDocument();
  });

  it('shows discovery interval for tailscale mode', () => {
    render(<NodeSettings {...defaults} bind="tailscale" />);
    expect(screen.getByLabelText('Discovery interval')).toBeInTheDocument();
  });

  it('shows discovery interval for public mode', () => {
    render(<NodeSettings {...defaults} bind="public" />);
    expect(screen.getByLabelText('Discovery interval')).toBeInTheDocument();
  });

  it('shows discovery interval for container mode', () => {
    render(<NodeSettings {...defaults} bind="container" />);
    expect(screen.getByLabelText('Discovery interval')).toBeInTheDocument();
  });

  it('calls onNameChange', () => {
    const onNameChange = vi.fn();
    render(<NodeSettings {...defaults} onNameChange={onNameChange} />);
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'new-name' } });
    expect(onNameChange).toHaveBeenCalledWith('new-name');
  });

  it('calls onPortChange with parsed integer', () => {
    const onPortChange = vi.fn();
    render(<NodeSettings {...defaults} onPortChange={onPortChange} />);
    fireEvent.change(screen.getByLabelText('Port'), { target: { value: '8080' } });
    expect(onPortChange).toHaveBeenCalledWith(8080);
  });

  it('calls onPortChange with 0 for invalid input', () => {
    const onPortChange = vi.fn();
    render(<NodeSettings {...defaults} onPortChange={onPortChange} />);
    fireEvent.change(screen.getByLabelText('Port'), { target: { value: 'abc' } });
    expect(onPortChange).toHaveBeenCalledWith(0);
  });

  it('calls onDataDirChange', () => {
    const onDataDirChange = vi.fn();
    render(<NodeSettings {...defaults} onDataDirChange={onDataDirChange} />);
    fireEvent.change(screen.getByLabelText('Data directory'), { target: { value: '/new/dir' } });
    expect(onDataDirChange).toHaveBeenCalledWith('/new/dir');
  });

  it('calls onTagChange', () => {
    const onTagChange = vi.fn();
    render(<NodeSettings {...defaults} onTagChange={onTagChange} />);
    fireEvent.change(screen.getByLabelText('Tag'), { target: { value: 'gpu' } });
    expect(onTagChange).toHaveBeenCalledWith('gpu');
  });

  it('calls onDiscoveryIntervalChange', () => {
    const onDiscoveryIntervalChange = vi.fn();
    render(
      <NodeSettings
        {...defaults}
        bind="public"
        onDiscoveryIntervalChange={onDiscoveryIntervalChange}
      />,
    );
    fireEvent.change(screen.getByLabelText('Discovery interval'), {
      target: { value: '120' },
    });
    expect(onDiscoveryIntervalChange).toHaveBeenCalledWith(120);
  });

  it('calls onDiscoveryIntervalChange with 0 for invalid input', () => {
    const onDiscoveryIntervalChange = vi.fn();
    render(
      <NodeSettings
        {...defaults}
        bind="public"
        onDiscoveryIntervalChange={onDiscoveryIntervalChange}
      />,
    );
    fireEvent.change(screen.getByLabelText('Discovery interval'), {
      target: { value: '' },
    });
    expect(onDiscoveryIntervalChange).toHaveBeenCalledWith(0);
  });

  it('shows bind mode description', () => {
    render(<NodeSettings {...defaults} bind="local" />);
    expect(screen.getByText(/Only reachable from this machine/)).toBeInTheDocument();
  });
});
