let toastMessage = $state('');
let toastVisible = $state(false);
let toastTimer: ReturnType<typeof setTimeout> | null = null;

export function getToastMessage(): string {
  return toastMessage;
}

export function isToastVisible(): boolean {
  return toastVisible;
}

export function showToast(message: string, duration = 3000): void {
  if (toastTimer) clearTimeout(toastTimer);
  toastMessage = message;
  toastVisible = true;
  toastTimer = setTimeout(() => {
    toastVisible = false;
    toastTimer = null;
  }, duration);
}

export function hideToast(): void {
  if (toastTimer) {
    clearTimeout(toastTimer);
    toastTimer = null;
  }
  toastVisible = false;
}
