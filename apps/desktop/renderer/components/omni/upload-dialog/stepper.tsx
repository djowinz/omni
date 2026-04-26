import * as React from 'react';

export interface StepperProps {
  steps: string[];
  current: number; // 0-indexed; the currently active step
  completed: number[]; // 0-indexed; steps that have been passed
  error: 'error' | 'warning' | null; // overrides the current pill style when set
}

export function Stepper({ steps, current, completed, error }: StepperProps) {
  return (
    <div className="flex items-center gap-2 text-xs">
      {steps.map((label, i) => {
        const isCompleted = completed.includes(i);
        const isCurrent = i === current;
        const isError = isCurrent && error === 'error';
        const isWarning = isCurrent && error === 'warning';

        const circleClass = isError
          ? 'bg-[#f43f5e] text-[#FAFAFA]'
          : isWarning
            ? 'bg-[#f59e0b] text-[#09090B]'
            : isCompleted || isCurrent
              ? 'bg-[#00D9FF] text-[#09090B]'
              : 'bg-[#27272A] text-[#71717a]';

        const labelClass = isError
          ? 'text-[#f43f5e]'
          : isWarning
            ? 'text-[#f59e0b]'
            : isCompleted || isCurrent
              ? 'text-[#00D9FF] font-semibold'
              : 'text-[#71717a]';

        const glyph = isError || isWarning ? '!' : isCompleted ? '✓' : String(i + 1);

        return (
          <React.Fragment key={i}>
            <div className="flex items-center gap-2">
              <div
                data-testid={`stepper-pill-${i}`}
                className={`flex h-6 w-6 items-center justify-center rounded-full font-bold ${circleClass}`}
              >
                {glyph}
              </div>
              <span className={labelClass}>{label}</span>
            </div>
            {i < steps.length - 1 && (
              <div className="h-px flex-1 bg-[#27272A]" style={{ maxWidth: 32 }} />
            )}
          </React.Fragment>
        );
      })}
    </div>
  );
}
