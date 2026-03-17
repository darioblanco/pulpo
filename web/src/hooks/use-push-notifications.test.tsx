import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { usePushNotifications, urlBase64ToUint8Array } from './use-push-notifications';

vi.mock('@/api/client', () => ({
  getVapidKey: vi.fn(),
  subscribePush: vi.fn(),
  unsubscribePush: vi.fn(),
}));

import { getVapidKey, subscribePush, unsubscribePush } from '@/api/client';

const mockGetVapidKey = vi.mocked(getVapidKey);
const mockSubscribePush = vi.mocked(subscribePush);
const mockUnsubscribePush = vi.mocked(unsubscribePush);

function createMockSubscription(endpoint = 'https://fcm.example.com/send/abc') {
  return {
    endpoint,
    toJSON: () => ({
      endpoint,
      keys: { p256dh: 'key-data', auth: 'auth-data' },
    }),
    unsubscribe: vi.fn().mockResolvedValue(true),
  };
}

function createMockPushManager(subscription: ReturnType<typeof createMockSubscription> | null) {
  return {
    getSubscription: vi.fn().mockResolvedValue(subscription),
    subscribe: vi.fn().mockResolvedValue(subscription),
  };
}

function createMockRegistration(
  pushManager: ReturnType<typeof createMockPushManager>,
): ServiceWorkerRegistration {
  return { pushManager } as unknown as ServiceWorkerRegistration;
}

let savedServiceWorker: PropertyDescriptor | undefined;
let savedPushManager: PropertyDescriptor | undefined;
let savedNotification: PropertyDescriptor | undefined;

function setupBrowserMocks(options: {
  supported?: boolean;
  permission?: NotificationPermission;
  subscription?: ReturnType<typeof createMockSubscription> | null;
  requestPermissionResult?: NotificationPermission;
}) {
  const {
    supported = true,
    permission = 'default',
    subscription = null,
    requestPermissionResult = 'granted',
  } = options;

  const pushManager = createMockPushManager(subscription);
  const registration = createMockRegistration(pushManager);

  if (supported) {
    Object.defineProperty(navigator, 'serviceWorker', {
      value: {
        ready: Promise.resolve(registration),
      },
      configurable: true,
      writable: true,
    });
    Object.defineProperty(window, 'PushManager', {
      value: class {},
      configurable: true,
      writable: true,
    });
  } else {
    // Remove serviceWorker to simulate unsupported browser
    Object.defineProperty(navigator, 'serviceWorker', {
      value: undefined,
      configurable: true,
      writable: true,
    });
    // Remove PushManager
    const desc = Object.getOwnPropertyDescriptor(window, 'PushManager');
    if (desc) {
      delete (window as unknown as Record<string, unknown>).PushManager;
    }
  }

  Object.defineProperty(window, 'Notification', {
    value: {
      permission,
      requestPermission: vi.fn().mockResolvedValue(requestPermissionResult),
    },
    configurable: true,
    writable: true,
  });

  return { pushManager, registration };
}

beforeEach(() => {
  vi.clearAllMocks();
  savedServiceWorker = Object.getOwnPropertyDescriptor(navigator, 'serviceWorker');
  savedPushManager = Object.getOwnPropertyDescriptor(window, 'PushManager');
  savedNotification = Object.getOwnPropertyDescriptor(window, 'Notification');
});

afterEach(() => {
  if (savedServiceWorker) {
    Object.defineProperty(navigator, 'serviceWorker', savedServiceWorker);
  }
  if (savedPushManager) {
    Object.defineProperty(window, 'PushManager', savedPushManager);
  } else {
    delete (window as unknown as Record<string, unknown>).PushManager;
  }
  if (savedNotification) {
    Object.defineProperty(window, 'Notification', savedNotification);
  }
});

describe('urlBase64ToUint8Array', () => {
  it('converts a base64url string to Uint8Array', () => {
    // "AQAB" in base64 is [1, 0, 1]
    const result = urlBase64ToUint8Array('AQAB');
    expect(result).toBeInstanceOf(Uint8Array);
    expect(result[0]).toBe(1);
    expect(result[1]).toBe(0);
    expect(result[2]).toBe(1);
  });

  it('handles base64url characters (- and _)', () => {
    // Base64url uses - instead of + and _ instead of /
    const result = urlBase64ToUint8Array('AP-_');
    expect(result).toBeInstanceOf(Uint8Array);
    expect(result.length).toBe(3);
  });

  it('adds padding when needed', () => {
    // "AQ" needs == padding to become "AQ=="
    const result = urlBase64ToUint8Array('AQ');
    expect(result).toBeInstanceOf(Uint8Array);
    expect(result[0]).toBe(1);
  });
});

describe('usePushNotifications', () => {
  it('reports unsupported when no serviceWorker', async () => {
    setupBrowserMocks({ supported: false });

    const { result } = renderHook(() => usePushNotifications());

    await waitFor(() => {
      expect(result.current.isSupported).toBe(false);
    });
    expect(result.current.isEnabled).toBe(false);
    expect(result.current.permission).toBe('default');
  });

  it('detects existing subscription on mount', async () => {
    const subscription = createMockSubscription();
    setupBrowserMocks({ supported: true, permission: 'granted', subscription });

    const { result } = renderHook(() => usePushNotifications());

    await waitFor(() => {
      expect(result.current.isSupported).toBe(true);
      expect(result.current.isEnabled).toBe(true);
    });
    expect(result.current.permission).toBe('granted');
  });

  it('detects no subscription on mount', async () => {
    setupBrowserMocks({ supported: true, permission: 'default', subscription: null });

    const { result } = renderHook(() => usePushNotifications());

    await waitFor(() => {
      expect(result.current.isSupported).toBe(true);
    });
    expect(result.current.isEnabled).toBe(false);
    expect(result.current.permission).toBe('default');
  });

  it('enable flow: permission granted, subscribes', async () => {
    const subscription = createMockSubscription();
    const { pushManager } = setupBrowserMocks({
      supported: true,
      permission: 'default',
      subscription: null,
      requestPermissionResult: 'granted',
    });
    pushManager.subscribe.mockResolvedValue(subscription);

    mockGetVapidKey.mockResolvedValue({ public_key: 'AQAB' });
    mockSubscribePush.mockResolvedValue(undefined);

    const { result } = renderHook(() => usePushNotifications());

    await waitFor(() => {
      expect(result.current.isSupported).toBe(true);
    });

    await act(async () => {
      await result.current.enable();
    });

    expect(mockGetVapidKey).toHaveBeenCalled();
    expect(pushManager.subscribe).toHaveBeenCalledWith({
      userVisibleOnly: true,
      applicationServerKey: expect.any(ArrayBuffer),
    });
    expect(mockSubscribePush).toHaveBeenCalledWith({
      endpoint: 'https://fcm.example.com/send/abc',
      keys: { p256dh: 'key-data', auth: 'auth-data' },
    });
    expect(result.current.isEnabled).toBe(true);
    expect(result.current.isLoading).toBe(false);
    expect(result.current.permission).toBe('granted');
  });

  it('enable flow: permission denied, does not subscribe', async () => {
    setupBrowserMocks({
      supported: true,
      permission: 'default',
      subscription: null,
      requestPermissionResult: 'denied',
    });

    const { result } = renderHook(() => usePushNotifications());

    await waitFor(() => {
      expect(result.current.isSupported).toBe(true);
    });

    await act(async () => {
      await result.current.enable();
    });

    expect(mockGetVapidKey).not.toHaveBeenCalled();
    expect(result.current.isEnabled).toBe(false);
    expect(result.current.isLoading).toBe(false);
    expect(result.current.permission).toBe('denied');
  });

  it('enable flow: handles error gracefully', async () => {
    setupBrowserMocks({
      supported: true,
      permission: 'default',
      subscription: null,
      requestPermissionResult: 'granted',
    });

    mockGetVapidKey.mockRejectedValue(new Error('Network error'));
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const { result } = renderHook(() => usePushNotifications());

    await waitFor(() => {
      expect(result.current.isSupported).toBe(true);
    });

    await act(async () => {
      await result.current.enable();
    });

    expect(consoleSpy).toHaveBeenCalledWith(
      'Failed to enable push notifications:',
      expect.any(Error),
    );
    expect(result.current.isEnabled).toBe(false);
    expect(result.current.isLoading).toBe(false);
    consoleSpy.mockRestore();
  });

  it('disable flow: unsubscribes existing subscription', async () => {
    const subscription = createMockSubscription();
    setupBrowserMocks({ supported: true, permission: 'granted', subscription });

    mockUnsubscribePush.mockResolvedValue(undefined);

    const { result } = renderHook(() => usePushNotifications());

    await waitFor(() => {
      expect(result.current.isEnabled).toBe(true);
    });

    await act(async () => {
      await result.current.disable();
    });

    expect(mockUnsubscribePush).toHaveBeenCalledWith('https://fcm.example.com/send/abc');
    expect(subscription.unsubscribe).toHaveBeenCalled();
    expect(result.current.isEnabled).toBe(false);
    expect(result.current.isLoading).toBe(false);
  });

  it('disable flow: handles no subscription gracefully', async () => {
    setupBrowserMocks({ supported: true, permission: 'granted', subscription: null });

    const { result } = renderHook(() => usePushNotifications());

    await waitFor(() => {
      expect(result.current.isSupported).toBe(true);
    });

    await act(async () => {
      await result.current.disable();
    });

    expect(mockUnsubscribePush).not.toHaveBeenCalled();
    expect(result.current.isEnabled).toBe(false);
    expect(result.current.isLoading).toBe(false);
  });

  it('disable flow: handles error gracefully', async () => {
    const subscription = createMockSubscription();
    setupBrowserMocks({ supported: true, permission: 'granted', subscription });

    mockUnsubscribePush.mockRejectedValue(new Error('Network error'));
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const { result } = renderHook(() => usePushNotifications());

    await waitFor(() => {
      expect(result.current.isEnabled).toBe(true);
    });

    await act(async () => {
      await result.current.disable();
    });

    expect(consoleSpy).toHaveBeenCalledWith(
      'Failed to disable push notifications:',
      expect.any(Error),
    );
    expect(result.current.isLoading).toBe(false);
    consoleSpy.mockRestore();
  });
});
