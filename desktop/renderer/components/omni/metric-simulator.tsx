

import { useOmniState } from '@/hooks/use-omni-state';
import { Slider } from '@/components/ui/slider';
import { Label } from '@/components/ui/label';
import { Activity, Cpu, MemoryStick, Gauge } from 'lucide-react';

interface MetricConfig {
  key: string;
  label: string;
  min: number;
  max: number;
  step: number;
  unit: string;
  category: 'fps' | 'gpu' | 'cpu' | 'ram';
}

const METRICS: MetricConfig[] = [
  { key: 'fps', label: 'FPS', min: 0, max: 300, step: 1, unit: '', category: 'fps' },
  { key: 'frametime', label: 'Frametime', min: 0, max: 100, step: 0.1, unit: 'ms', category: 'fps' },
  { key: 'frame.1pct', label: 'Frame 1%', min: 0, max: 200, step: 1, unit: '', category: 'fps' },
  { key: 'gpu.usage', label: 'GPU Usage', min: 0, max: 100, step: 1, unit: '%', category: 'gpu' },
  { key: 'gpu.temp', label: 'GPU Temp', min: 20, max: 100, step: 1, unit: '°C', category: 'gpu' },
  { key: 'gpu.clock', label: 'GPU Clock', min: 500, max: 3000, step: 10, unit: 'MHz', category: 'gpu' },
  { key: 'gpu.vram.used', label: 'VRAM Used', min: 0, max: 24000, step: 100, unit: 'MB', category: 'gpu' },
  { key: 'gpu.power', label: 'GPU Power', min: 0, max: 500, step: 5, unit: 'W', category: 'gpu' },
  { key: 'gpu.volt', label: 'GPU Voltage', min: 800, max: 1200, step: 10, unit: 'mV', category: 'gpu' },
  { key: 'gpu.fan', label: 'GPU Fan', min: 0, max: 100, step: 1, unit: '%', category: 'gpu' },
  { key: 'cpu.usage', label: 'CPU Usage', min: 0, max: 100, step: 1, unit: '%', category: 'cpu' },
  { key: 'cpu.temp', label: 'CPU Temp', min: 20, max: 100, step: 1, unit: '°C', category: 'cpu' },
  { key: 'ram.usage', label: 'RAM Usage', min: 0, max: 100, step: 1, unit: '%', category: 'ram' },
];

const categoryColors = {
  fps: '#00D9FF',
  gpu: '#A855F7',
  cpu: '#3B82F6',
  ram: '#22C55E',
};

const categoryIcons = {
  fps: Activity,
  gpu: Gauge,
  cpu: Cpu,
  ram: MemoryStick,
};

export function MetricSimulator() {
  const { state, dispatch } = useOmniState();

  const handleMetricChange = (key: string, value: number[]) => {
    dispatch({
      type: 'UPDATE_PREVIEW_METRIC',
      payload: { key, value: value[0] },
    });
  };

  const getMetricValue = (key: string): number => {
    const metrics = state.previewMetrics as Record<string, unknown>;
    const value = metrics[key];
    return typeof value === 'number' ? value : 0;
  };

  return (
    <div className="p-3">
      <div className="flex items-center gap-2 mb-3">
        <Activity className="h-3.5 w-3.5 text-[#00D9FF]" />
        <h3 className="text-[10px] font-semibold uppercase tracking-widest text-[#71717A]">
          Metric Simulator
        </h3>
      </div>
      <div className="grid grid-cols-2 gap-x-4 gap-y-2.5">
        {METRICS.map(metric => {
          const color = categoryColors[metric.category];
          const value = getMetricValue(metric.key);
          
          return (
            <div key={metric.key} className="flex flex-col gap-1">
              <div className="flex items-center justify-between">
                <Label className="text-[10px] text-[#71717A] uppercase tracking-wide">{metric.label}</Label>
                <span 
                  className="text-xs font-mono font-medium"
                  style={{ color }}
                >
                  {value.toFixed(metric.step < 1 ? 1 : 0)}
                  <span className="text-[#52525B] ml-0.5">{metric.unit}</span>
                </span>
              </div>
              <Slider
                value={[value]}
                min={metric.min}
                max={metric.max}
                step={metric.step}
                onValueChange={val => handleMetricChange(metric.key, val)}
                className="w-full [&_[role=slider]]:bg-[#27272A] [&_[role=slider]]:border-none [&_[role=slider]]:h-3 [&_[role=slider]]:w-3"
                style={{
                  // @ts-expect-error CSS custom property
                  '--slider-track': '#27272A',
                  '--slider-range': color,
                }}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}
