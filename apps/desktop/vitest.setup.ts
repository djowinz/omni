import { expect } from 'vitest';
import * as matchers from '@testing-library/jest-dom/matchers';

expect.extend(matchers);

// Radix UI primitives (Select, Dialog, etc.) use Pointer Events and
// scrollIntoView APIs that jsdom does not implement. Polyfill them so that
// Radix components open/close correctly in unit tests.
if (typeof window !== 'undefined') {
  window.HTMLElement.prototype.hasPointerCapture = (_pointerId: number) => false;
  window.HTMLElement.prototype.setPointerCapture = (_pointerId: number) => {};
  window.HTMLElement.prototype.releasePointerCapture = (_pointerId: number) => {};
  window.HTMLElement.prototype.scrollIntoView = () => {};
}
