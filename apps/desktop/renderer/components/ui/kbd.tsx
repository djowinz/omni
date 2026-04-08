import { cn } from '@/lib/utils';

interface KbdProps {
  children: React.ReactNode;
  className?: string;
}

export function Kbd({ children, className }: KbdProps) {
  return (
    <kbd
      className={cn(
        'inline-flex items-center justify-center rounded border border-[#3f3f46] border-b-2 border-b-[#27272A] bg-[#0D0D0F] px-2 py-0.5 font-mono text-xs font-semibold text-[#FAFAFA] shadow-sm',
        className,
      )}
    >
      {children}
    </kbd>
  );
}
