import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { ConnectForm } from './connect-form';
import * as connection from '@/api/connection';

vi.mock('@/api/connection', () => ({
  testConnection: vi.fn(),
}));

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

const mockTestConnection = vi.mocked(connection.testConnection);

beforeEach(() => {
  mockTestConnection.mockReset();
  mockFetch.mockReset();
  // Default: health probe succeeds
  mockFetch.mockResolvedValue({ ok: true });
});

describe('ConnectForm', () => {
  it('renders url and token fields', () => {
    render(<ConnectForm onConnect={vi.fn()} />);
    expect(screen.getByTestId('connect-form')).toBeInTheDocument();
    expect(screen.getByLabelText('Node URL')).toBeInTheDocument();
    expect(screen.getByLabelText('Auth token')).toBeInTheDocument();
    expect(screen.getByTestId('connect-btn')).toBeInTheDocument();
  });

  it('defaults url to localhost:7433', () => {
    render(<ConnectForm onConnect={vi.fn()} />);
    expect(screen.getByLabelText('Node URL')).toHaveValue('http://localhost:7433');
  });

  it('does not connect with empty url', () => {
    render(<ConnectForm onConnect={vi.fn()} />);
    fireEvent.change(screen.getByLabelText('Node URL'), { target: { value: '' } });
    fireEvent.click(screen.getByTestId('connect-btn'));
    expect(mockTestConnection).not.toHaveBeenCalled();
  });

  it('connects successfully', async () => {
    const node = {
      name: 'mac-studio',
      hostname: 'mac',
      os: 'darwin',
      arch: 'arm64',
      cpus: 8,
      memory_mb: 32000,
      gpu: null,
    };
    mockTestConnection.mockResolvedValue(node);
    const onConnect = vi.fn();
    render(<ConnectForm onConnect={onConnect} />);

    fireEvent.change(screen.getByLabelText('Node URL'), {
      target: { value: 'http://10.0.0.1:7433' },
    });
    fireEvent.change(screen.getByLabelText('Auth token'), { target: { value: 'secret' } });
    fireEvent.click(screen.getByTestId('connect-btn'));

    await waitFor(() => {
      expect(mockTestConnection).toHaveBeenCalledWith('http://10.0.0.1:7433', 'secret');
      expect(onConnect).toHaveBeenCalledWith('http://10.0.0.1:7433', 'secret', 'mac-studio');
    });
  });

  it('connects without token', async () => {
    const node = {
      name: 'node',
      hostname: 'h',
      os: 'linux',
      arch: 'x86_64',
      cpus: 4,
      memory_mb: 16000,
      gpu: null,
    };
    mockTestConnection.mockResolvedValue(node);
    const onConnect = vi.fn();
    render(<ConnectForm onConnect={onConnect} />);

    fireEvent.click(screen.getByTestId('connect-btn'));

    await waitFor(() => {
      expect(mockTestConnection).toHaveBeenCalledWith('http://localhost:7433', undefined);
      expect(onConnect).toHaveBeenCalledWith('http://localhost:7433', '', 'node');
    });
  });

  it('shows error on connection failure', async () => {
    mockTestConnection.mockRejectedValue(new Error('Connection refused'));
    render(<ConnectForm onConnect={vi.fn()} />);

    fireEvent.click(screen.getByTestId('connect-btn'));

    await waitFor(() => {
      expect(screen.getByText('Connection refused')).toBeInTheDocument();
    });
  });

  it('shows generic error for non-Error failure', async () => {
    mockTestConnection.mockRejectedValue('string error');
    render(<ConnectForm onConnect={vi.fn()} />);

    fireEvent.click(screen.getByTestId('connect-btn'));

    await waitFor(() => {
      expect(screen.getByText('Connection failed')).toBeInTheDocument();
    });
  });

  it('uses initialToken when provided', () => {
    render(<ConnectForm onConnect={vi.fn()} initialToken="pre-filled" />);
    expect(screen.getByLabelText('Auth token')).toHaveValue('pre-filled');
  });

  it('shows online status when health check succeeds', async () => {
    mockFetch.mockResolvedValue({ ok: true });
    render(<ConnectForm onConnect={vi.fn()} />);

    await waitFor(() => {
      expect(screen.getByTestId('node-status')).toHaveTextContent('Online');
    });
    expect(mockFetch).toHaveBeenCalledWith('http://localhost:7433/api/v1/health');
  });

  it('shows offline status when health check fails', async () => {
    mockFetch.mockResolvedValue({ ok: false });
    render(<ConnectForm onConnect={vi.fn()} />);

    await waitFor(() => {
      expect(screen.getByTestId('node-status')).toHaveTextContent('Offline');
    });
  });

  it('shows offline status on network error', async () => {
    mockFetch.mockRejectedValue(new Error('Network error'));
    render(<ConnectForm onConnect={vi.fn()} />);

    await waitFor(() => {
      expect(screen.getByTestId('node-status')).toHaveTextContent('Offline');
    });
  });

  it('hides status indicator when url is empty', async () => {
    render(<ConnectForm onConnect={vi.fn()} />);
    fireEvent.change(screen.getByLabelText('Node URL'), { target: { value: '' } });

    await waitFor(() => {
      expect(screen.queryByTestId('node-status')).not.toBeInTheDocument();
    });
  });

  it('re-probes when url changes', async () => {
    mockFetch.mockResolvedValue({ ok: true });
    render(<ConnectForm onConnect={vi.fn()} />);

    await waitFor(() => {
      expect(screen.getByTestId('node-status')).toHaveTextContent('Online');
    });

    mockFetch.mockResolvedValue({ ok: false });
    fireEvent.change(screen.getByLabelText('Node URL'), {
      target: { value: 'http://other:7433' },
    });

    await waitFor(() => {
      expect(screen.getByTestId('node-status')).toHaveTextContent('Offline');
    });
  });
});
