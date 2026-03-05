import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

// Auto-cleanup after each test (RTL requires globals: true or explicit cleanup)
afterEach(cleanup);

// jsdom doesn't implement matchMedia — polyfill for shadcn sidebar tests
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  }),
});

// jsdom doesn't implement scrollIntoView — stub for output-view tests
Element.prototype.scrollIntoView = () => {};

// jsdom doesn't implement pointer capture — stub for Radix Select tests
Element.prototype.hasPointerCapture = () => false;
Element.prototype.setPointerCapture = () => {};
Element.prototype.releasePointerCapture = () => {};
