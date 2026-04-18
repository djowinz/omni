import * as React from 'react';

import { cn } from '@/lib/utils';

export type PasswordStrength = 'none' | 'weak' | 'medium' | 'strong';

export function computeStrength(value: string, minLength: number): PasswordStrength {
  if (value.length === 0) return 'none';
  const classes =
    Number(/[a-z]/.test(value)) +
    Number(/[A-Z]/.test(value)) +
    Number(/[0-9]/.test(value)) +
    Number(/[^a-zA-Z0-9]/.test(value));
  if (value.length < minLength || classes <= 1) return 'weak';
  if (value.length >= minLength + 4 && classes === 4) return 'strong';
  return 'medium';
}

interface PasswordStrengthMeterProps {
  value: string;
  minLength?: number;
  className?: string;
}

const SEGMENT_COUNT: Record<PasswordStrength, number> = { none: 0, weak: 1, medium: 2, strong: 3 };
const LABEL: Record<PasswordStrength, string> = {
  none: '',
  weak: 'Weak',
  medium: 'Medium',
  strong: 'Strong',
};
const FILL_CLASS: Record<PasswordStrength, string> = {
  none: '',
  weak: 'bg-destructive',
  medium: 'bg-yellow-500',
  strong: 'bg-emerald-500',
};
const LABEL_CLASS: Record<PasswordStrength, string> = {
  none: 'text-muted-foreground',
  weak: 'text-destructive',
  medium: 'text-yellow-600 dark:text-yellow-500',
  strong: 'text-emerald-600 dark:text-emerald-500',
};

export function PasswordStrengthMeter({
  value,
  minLength = 12,
  className,
}: PasswordStrengthMeterProps) {
  const strength = computeStrength(value, minLength);
  const filled = SEGMENT_COUNT[strength];
  return (
    <div className={cn('flex flex-col gap-1', className)} data-slot="password-strength-meter">
      <div className="flex gap-1" aria-hidden="true">
        {[0, 1, 2].map((i) => (
          <div
            key={i}
            className={cn('h-1 flex-1 rounded-sm bg-muted', i < filled && FILL_CLASS[strength])}
          />
        ))}
      </div>
      <span
        className={cn('text-xs font-medium', LABEL_CLASS[strength])}
        aria-live="polite"
        role="status"
      >
        {LABEL[strength]}
      </span>
    </div>
  );
}
