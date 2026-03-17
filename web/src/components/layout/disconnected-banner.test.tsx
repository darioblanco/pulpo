import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { DisconnectedBanner } from './disconnected-banner';

const mockUseSSE = vi.fn();

vi.mock('@/hooks/use-sse', () => ({
  useSSE: () => mockUseSSE(),
}));

describe('DisconnectedBanner', () => {
  it('shows banner when disconnected', () => {
    mockUseSSE.mockReturnValue({ connected: false, sessions: [], setSessions: vi.fn() });
    render(<DisconnectedBanner />);
    expect(screen.getByTestId('disconnected-banner')).toBeInTheDocument();
    expect(screen.getByText(/Disconnected from pulpod/)).toBeInTheDocument();
  });

  it('hides banner when connected', () => {
    mockUseSSE.mockReturnValue({ connected: true, sessions: [], setSessions: vi.fn() });
    render(<DisconnectedBanner />);
    expect(screen.queryByTestId('disconnected-banner')).not.toBeInTheDocument();
  });

  it('shows pulsing dot indicator', () => {
    mockUseSSE.mockReturnValue({ connected: false, sessions: [], setSessions: vi.fn() });
    render(<DisconnectedBanner />);
    const dot = screen.getByTestId('disconnected-banner').querySelector('.animate-pulse');
    expect(dot).toBeInTheDocument();
  });
});
