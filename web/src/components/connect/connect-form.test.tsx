import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { ConnectForm } from './connect-form';
import * as connection from '@/api/connection';

vi.mock('@/api/connection', () => ({
  testConnection: vi.fn(),
}));

const mockTestConnection = vi.mocked(connection.testConnection);

beforeEach(() => {
  mockTestConnection.mockReset();
});

describe('ConnectForm', () => {
  it('renders url and token fields', () => {
    render(<ConnectForm onConnect={vi.fn()} />);
    expect(screen.getByTestId('connect-form')).toBeInTheDocument();
    expect(screen.getByLabelText('Node URL')).toBeInTheDocument();
    expect(screen.getByLabelText('Auth token')).toBeInTheDocument();
    expect(screen.getByTestId('connect-btn')).toBeInTheDocument();
  });

  it('does not connect with empty url', () => {
    render(<ConnectForm onConnect={vi.fn()} />);
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

    fireEvent.change(screen.getByLabelText('Node URL'), {
      target: { value: 'http://10.0.0.1:7433' },
    });
    fireEvent.click(screen.getByTestId('connect-btn'));

    await waitFor(() => {
      expect(mockTestConnection).toHaveBeenCalledWith('http://10.0.0.1:7433', undefined);
      expect(onConnect).toHaveBeenCalledWith('http://10.0.0.1:7433', '', 'node');
    });
  });

  it('shows error on connection failure', async () => {
    mockTestConnection.mockRejectedValue(new Error('Connection refused'));
    render(<ConnectForm onConnect={vi.fn()} />);

    fireEvent.change(screen.getByLabelText('Node URL'), { target: { value: 'http://bad:7433' } });
    fireEvent.click(screen.getByTestId('connect-btn'));

    await waitFor(() => {
      expect(screen.getByText('Connection refused')).toBeInTheDocument();
    });
  });

  it('shows generic error for non-Error failure', async () => {
    mockTestConnection.mockRejectedValue('string error');
    render(<ConnectForm onConnect={vi.fn()} />);

    fireEvent.change(screen.getByLabelText('Node URL'), { target: { value: 'http://bad:7433' } });
    fireEvent.click(screen.getByTestId('connect-btn'));

    await waitFor(() => {
      expect(screen.getByText('Connection failed')).toBeInTheDocument();
    });
  });

  it('uses initialToken when provided', () => {
    render(<ConnectForm onConnect={vi.fn()} initialToken="pre-filled" />);
    expect(screen.getByLabelText('Auth token')).toHaveValue('pre-filled');
  });
});
