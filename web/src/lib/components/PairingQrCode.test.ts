import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import PairingQrCode from './PairingQrCode.svelte';

const mockGetPairingUrl = vi.fn();
vi.mock('$lib/api', () => ({
  getPairingUrl: (...args: unknown[]) => mockGetPairingUrl(...args),
}));

const mockToString = vi.fn();
vi.mock('qrcode', () => ({
  default: {
    toString: (...args: unknown[]) => mockToString(...args),
  },
}));

afterEach(() => {
  cleanup();
  mockGetPairingUrl.mockReset();
  mockToString.mockReset();
});

describe('PairingQrCode', () => {
  it('renders QR code from pairing URL', async () => {
    mockGetPairingUrl.mockResolvedValue({ url: 'http://mac-mini:7433/?token=abc123' });
    mockToString.mockResolvedValue('<svg>QR</svg>');

    render(PairingQrCode);

    await waitFor(() => {
      expect(screen.getByText('http://mac-mini:7433/?token=abc123')).toBeTruthy();
    });

    expect(mockGetPairingUrl).toHaveBeenCalled();
    expect(mockToString).toHaveBeenCalledWith('http://mac-mini:7433/?token=abc123', {
      type: 'svg',
      margin: 1,
    });
  });

  it('shows error on failure', async () => {
    mockGetPairingUrl.mockRejectedValue(new Error('Network error'));

    render(PairingQrCode);

    await waitFor(() => {
      expect(screen.getByText('Failed to generate pairing code')).toBeTruthy();
    });
  });
});
