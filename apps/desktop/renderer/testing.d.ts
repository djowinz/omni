// Extend Vitest's Assertion with @testing-library/jest-dom matchers.
//
// WHY THIS FILE EXISTS:
// @testing-library/jest-dom declares a peer dep of vitest ^0.34.x.
// pnpm resolves that peer to vitest@2.x (in the shared store), so
// jest-dom/vitest augments the WRONG vitest version's Assertion.
// This file re-declares the augmentation against the project-local
// vitest@4.x package.
//
// The side-effect import below makes this a "module" file so that
// `declare module 'vitest'` is a module augmentation (not a replacement).
import 'vitest';

declare module 'vitest' {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  interface Assertion<R = any> {
    // Presence / absence
    toBeInTheDocument(): R;
    toBeInTheDOM(container?: HTMLElement | SVGElement): R;
    // State
    toBeVisible(): R;
    toBeEnabled(): R;
    toBeDisabled(): R;
    toBeRequired(): R;
    toBeInvalid(): R;
    toBeValid(): R;
    toBeEmptyDOMElement(): R;
    toBeChecked(): R;
    toBePartiallyChecked(): R;
    toBePressed(): R;
    toBePartiallyPressed(): R;
    // Content
    toHaveTextContent(text: string | RegExp, options?: { normalizeWhitespace: boolean }): R;
    toHaveValue(value?: string | string[] | number | null): R;
    toHaveDisplayValue(value: string | RegExp | Array<string | RegExp>): R;
    toHaveFormValues(expectedValues: Record<string, unknown>): R;
    // Attributes / classes / styles / roles
    toHaveAttribute(attr: string, value?: string | RegExp | null): R;
    toHaveClass(...classNames: string[]): R;
    toHaveClass(className: string, options?: { exact: boolean }): R;
    toHaveStyle(css: string | Record<string, string>): R;
    toHaveRole(role: string): R;
    // Focus
    toHaveFocus(): R;
    // Containment
    toContainElement(element: HTMLElement | SVGElement | null): R;
    toContainHTML(htmlText: string): R;
    // Accessible names / descriptions
    toHaveAccessibleName(name?: string | RegExp): R;
    toHaveAccessibleDescription(description?: string | RegExp): R;
    toHaveAccessibleErrorMessage(message?: string | RegExp): R;
    // Selection
    toHaveSelection(selection?: string): R;
    // Description (deprecated)
    toHaveDescription(text?: string | RegExp): R;
    toHaveErrorMessage(text?: string | RegExp): R;
    // Timing
    toAppearBefore(element: HTMLElement | SVGElement): R;
    toAppearAfter(element: HTMLElement | SVGElement): R;
  }
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  interface AsymmetricMatchersContaining {
    toBeInTheDocument(): void;
    toBeVisible(): void;
    toHaveTextContent(text: string | RegExp, options?: { normalizeWhitespace: boolean }): void;
    toHaveAttribute(attr: string, value?: string | RegExp | null): void;
    toHaveClass(...classNames: string[]): void;
    toHaveStyle(css: string | Record<string, string>): void;
    toContainElement(element: HTMLElement | SVGElement | null): void;
    toContainHTML(htmlText: string): void;
  }
}
