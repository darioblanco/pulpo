import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { ConnectionProvider } from '@/hooks/use-connection';
import { ConnectPage } from './connect';
import * as connection from '@/api/connection';

vi.mock('@/api/connection', () => ({
  testConnection: vi.fn(),
}));

vi.mock('@/api/client', () => ({
  setApiConfig: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
}));

const mockNavigate = vi.fn();
vi.mock('react-router', async () => {
  const actual = await vi.importActual('react-router');
  return {
    ...actual,
    useNavigate: () => mockNavigate,
  };
});

const mockTestConnection = vi.mocked(connection.testConnection);

const localStorageStore: Record<string, string> = {};
vi.stubGlobal('localStorage', {
  getItem: (key: string) => localStorageStore[key] ?? null,
  setItem: (key: string, value: string) => {
    localStorageStore[key] = value;
  },
  removeItem: (key: string) => {
    delete localStorageStore[key];
  },
});

beforeEach(() => {
  mockTestConnection.mockReset();
  mockNavigate.mockReset();
  for (const key of Object.keys(localStorageStore)) delete localStorageStore[key];
});

function renderConnect(initialEntries: string[] = ['/connect']) {
  return render(
    <MemoryRouter initialEntries={initialEntries}>
      <ConnectionProvider>
        <ConnectPage />
      </ConnectionProvider>
    </MemoryRouter>,
  );
}

describe('ConnectPage', () => {
  it('renders connect form', () => {
    renderConnect();
    expect(screen.getByTestId('connect-page')).toBeInTheDocument();
    expect(screen.getByTestId('connect-form')).toBeInTheDocument();
    expect(screen.getByText('Connect to Pulpo')).toBeInTheDocument();
  });

  it('connects and navigates to dashboard', async () => {
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
    renderConnect();

    fireEvent.change(screen.getByLabelText('Node URL'), {
      target: { value: 'http://10.0.0.1:7433' },
    });
    fireEvent.click(screen.getByTestId('connect-btn'));

    await waitFor(() => {
      expect(mockNavigate).toHaveBeenCalledWith('/');
    });
  });

  it('picks up token from URL params', () => {
    renderConnect(['/connect?token=url-token']);
    expect(screen.getByLabelText('Auth token')).toHaveValue('url-token');
  });

  it('shows saved connections and reconnects', async () => {
    localStorageStore['pulpo:connections'] = JSON.stringify([
      { name: 'saved-node', url: 'http://saved:7433', lastConnected: '2026-01-01T00:00:00Z' },
    ]);
    renderConnect();

    await waitFor(() => {
      expect(screen.getByTestId('saved-connections')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('select-saved-node'));

    await waitFor(() => {
      expect(mockNavigate).toHaveBeenCalledWith('/');
    });
  });

  it('removes a saved connection', async () => {
    localStorageStore['pulpo:connections'] = JSON.stringify([
      { name: 'old-node', url: 'http://old:7433', lastConnected: '2026-01-01T00:00:00Z' },
    ]);
    renderConnect();

    await waitFor(() => {
      expect(screen.getByTestId('saved-old-node')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('remove-old-node'));

    await waitFor(() => {
      expect(screen.queryByTestId('saved-old-node')).not.toBeInTheDocument();
    });
  });
});
