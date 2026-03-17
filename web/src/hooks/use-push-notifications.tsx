import { useState, useEffect, useCallback } from 'react';
import { getVapidKey, subscribePush, unsubscribePush } from '@/api/client';

function urlBase64ToUint8Array(base64String: string): Uint8Array {
  const padding = '='.repeat((4 - (base64String.length % 4)) % 4);
  const base64 = (base64String + padding).replace(/-/g, '+').replace(/_/g, '/');
  const rawData = atob(base64);
  const outputArray = new Uint8Array(rawData.length);
  for (let i = 0; i < rawData.length; ++i) {
    outputArray[i] = rawData.charCodeAt(i);
  }
  return outputArray;
}

export { urlBase64ToUint8Array };

export function usePushNotifications() {
  const [isSupported, setIsSupported] = useState(false);
  const [isEnabled, setIsEnabled] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [permission, setPermission] = useState<NotificationPermission>('default');

  useEffect(() => {
    const supported = 'serviceWorker' in navigator && 'PushManager' in window;
    setIsSupported(supported);
    if (supported) {
      setPermission(Notification.permission);
      navigator.serviceWorker.ready.then((reg) => {
        reg.pushManager.getSubscription().then((sub) => {
          setIsEnabled(!!sub);
        });
      });
    }
  }, []);

  const enable = useCallback(async () => {
    setIsLoading(true);
    try {
      const perm = await Notification.requestPermission();
      setPermission(perm);
      if (perm !== 'granted') return;

      const { public_key } = await getVapidKey();
      const reg = await navigator.serviceWorker.ready;
      const subscription = await reg.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: urlBase64ToUint8Array(public_key).buffer as ArrayBuffer,
      });

      const json = subscription.toJSON();
      await subscribePush(json);
      setIsEnabled(true);
    } catch (err) {
      console.error('Failed to enable push notifications:', err);
    } finally {
      setIsLoading(false);
    }
  }, []);

  const disable = useCallback(async () => {
    setIsLoading(true);
    try {
      const reg = await navigator.serviceWorker.ready;
      const subscription = await reg.pushManager.getSubscription();
      if (subscription) {
        await unsubscribePush(subscription.endpoint);
        await subscription.unsubscribe();
      }
      setIsEnabled(false);
    } catch (err) {
      console.error('Failed to disable push notifications:', err);
    } finally {
      setIsLoading(false);
    }
  }, []);

  return { isSupported, isEnabled, isLoading, permission, enable, disable };
}
