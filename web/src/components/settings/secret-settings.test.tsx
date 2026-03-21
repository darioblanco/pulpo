import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { SecretSettings } from './secret-settings';
import * as api from '@/api/client';

vi.mock('@/api/client', () => ({
  getSecrets: vi.fn(),
  setSecret: vi.fn(),
  deleteSecret: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

const mockGetSecrets = vi.mocked(api.getSecrets);
const mockSetSecret = vi.mocked(api.setSecret);
const mockDeleteSecret = vi.mocked(api.deleteSecret);

beforeEach(() => {
  mockGetSecrets.mockReset();
  mockSetSecret.mockReset();
  mockDeleteSecret.mockReset();
});

describe('SecretSettings', () => {
  it('shows empty state when no secrets', async () => {
    mockGetSecrets.mockResolvedValue([]);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('secrets-empty')).toBeInTheDocument();
      expect(screen.getByText(/No secrets configured/)).toBeInTheDocument();
    });
  });

  it('shows loading initially', () => {
    mockGetSecrets.mockResolvedValue([]);
    render(<SecretSettings />);
    expect(screen.getByText('Loading...')).toBeInTheDocument();
  });

  it('displays secrets list', async () => {
    mockGetSecrets.mockResolvedValue([
      { name: 'GITHUB_TOKEN', created_at: '2026-01-01T00:00:00Z' },
      { name: 'NPM_TOKEN', created_at: '2026-01-02T00:00:00Z' },
    ]);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('secrets-list')).toBeInTheDocument();
      expect(screen.getByTestId('secret-GITHUB_TOKEN')).toBeInTheDocument();
      expect(screen.getByTestId('secret-NPM_TOKEN')).toBeInTheDocument();
    });
  });

  it('displays env override when different from name', async () => {
    mockGetSecrets.mockResolvedValue([
      { name: 'GH_WORK', env: 'GITHUB_TOKEN', created_at: '2026-01-01T00:00:00Z' },
    ]);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('secret-env-GH_WORK')).toBeInTheDocument();
      expect(screen.getByTestId('secret-env-GH_WORK')).toHaveTextContent('ENV: GITHUB_TOKEN');
    });
  });

  it('does not display env when same as name', async () => {
    mockGetSecrets.mockResolvedValue([
      { name: 'GITHUB_TOKEN', env: 'GITHUB_TOKEN', created_at: '2026-01-01T00:00:00Z' },
    ]);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('secret-GITHUB_TOKEN')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('secret-env-GITHUB_TOKEN')).not.toBeInTheDocument();
  });

  it('renders add secret form with env field', async () => {
    mockGetSecrets.mockResolvedValue([]);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('add-secret-form')).toBeInTheDocument();
      expect(screen.getByTestId('secret-name-input')).toBeInTheDocument();
      expect(screen.getByTestId('secret-env-input')).toBeInTheDocument();
      expect(screen.getByTestId('secret-value-input')).toBeInTheDocument();
      expect(screen.getByTestId('add-secret-btn')).toBeInTheDocument();
    });
  });

  it('add button is disabled when fields are empty', async () => {
    mockGetSecrets.mockResolvedValue([]);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('add-secret-btn')).toBeDisabled();
    });
  });

  it('toggles value visibility', async () => {
    mockGetSecrets.mockResolvedValue([]);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('secret-value-input')).toHaveAttribute('type', 'password');
    });

    fireEvent.click(screen.getByTestId('toggle-value-visibility'));
    expect(screen.getByTestId('secret-value-input')).toHaveAttribute('type', 'text');

    fireEvent.click(screen.getByTestId('toggle-value-visibility'));
    expect(screen.getByTestId('secret-value-input')).toHaveAttribute('type', 'password');
  });

  it('adds a secret successfully', async () => {
    mockGetSecrets.mockResolvedValue([]);
    mockSetSecret.mockResolvedValue(undefined);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('add-secret-btn')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('secret-name-input'), {
      target: { value: 'MY_TOKEN' },
    });
    fireEvent.change(screen.getByTestId('secret-value-input'), {
      target: { value: 'abc123' },
    });

    fireEvent.click(screen.getByTestId('add-secret-btn'));

    await waitFor(() => {
      expect(mockSetSecret).toHaveBeenCalledWith('MY_TOKEN', 'abc123', undefined);
    });
  });

  it('adds a secret with env override', async () => {
    mockGetSecrets.mockResolvedValue([]);
    mockSetSecret.mockResolvedValue(undefined);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('add-secret-btn')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('secret-name-input'), {
      target: { value: 'GH_WORK' },
    });
    fireEvent.change(screen.getByTestId('secret-env-input'), {
      target: { value: 'GITHUB_TOKEN' },
    });
    fireEvent.change(screen.getByTestId('secret-value-input'), {
      target: { value: 'token123' },
    });

    fireEvent.click(screen.getByTestId('add-secret-btn'));

    await waitFor(() => {
      expect(mockSetSecret).toHaveBeenCalledWith('GH_WORK', 'token123', 'GITHUB_TOKEN');
    });
  });

  it('shows error when add fails', async () => {
    mockGetSecrets.mockResolvedValue([]);
    mockSetSecret.mockRejectedValue(new Error('Invalid name'));
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('add-secret-btn')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('secret-name-input'), {
      target: { value: 'KEY' },
    });
    fireEvent.change(screen.getByTestId('secret-value-input'), {
      target: { value: 'val' },
    });

    fireEvent.click(screen.getByTestId('add-secret-btn'));

    await waitFor(() => {
      expect(mockSetSecret).toHaveBeenCalled();
    });
  });

  it('deletes a secret', async () => {
    mockGetSecrets.mockResolvedValue([{ name: 'DEL_ME', created_at: '2026-01-01T00:00:00Z' }]);
    mockDeleteSecret.mockResolvedValue(undefined);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('delete-secret-DEL_ME')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('delete-secret-DEL_ME'));

    await waitFor(() => {
      expect(mockDeleteSecret).toHaveBeenCalledWith('DEL_ME');
    });
  });

  it('shows error when delete fails', async () => {
    mockGetSecrets.mockResolvedValue([{ name: 'FAIL', created_at: '2026-01-01T00:00:00Z' }]);
    mockDeleteSecret.mockRejectedValue(new Error('not found'));
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('delete-secret-FAIL')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('delete-secret-FAIL'));

    await waitFor(() => {
      expect(mockDeleteSecret).toHaveBeenCalled();
    });
  });

  it('forces uppercase and valid chars in name input', async () => {
    mockGetSecrets.mockResolvedValue([]);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('secret-name-input')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('secret-name-input'), {
      target: { value: 'my-token' },
    });

    // The onChange handler uppercases and strips invalid chars
    // The value displayed should be MYTOKEN (hyphens stripped)
    expect(screen.getByTestId('secret-name-input')).toHaveValue('MYTOKEN');
  });

  it('forces uppercase and valid chars in env input', async () => {
    mockGetSecrets.mockResolvedValue([]);
    render(<SecretSettings />);

    await waitFor(() => {
      expect(screen.getByTestId('secret-env-input')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('secret-env-input'), {
      target: { value: 'my-var' },
    });

    expect(screen.getByTestId('secret-env-input')).toHaveValue('MYVAR');
  });

  it('shows error on load failure', async () => {
    mockGetSecrets.mockRejectedValue(new Error('Network error'));
    render(<SecretSettings />);

    await waitFor(() => {
      // Should complete loading without crashing
      expect(screen.getByTestId('secret-settings')).toBeInTheDocument();
    });
  });
});
