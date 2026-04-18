import type { NextConfig } from 'next';
import { execSync } from 'child_process';
import path from 'path';

// GitDate version: Year.Month.Day.GitTag
function getGitDateVersion(): string {
  const opts = { encoding: 'utf-8' as const, cwd: path.resolve(__dirname, '../../..') };
  try {
    const date = execSync('git log -1 --format=%cs', opts).trim();
    let tag: string;
    try {
      tag = execSync('git describe --tags --abbrev=0', opts).trim();
    } catch {
      tag = '0';
    }
    const cleanTag = tag.replace(/^v/, '').replace(/\./g, '');
    return `${date.replace(/-/g, '.')}.${cleanTag}`;
  } catch {
    return `${new Date().toISOString().split('T')[0].replace(/-/g, '.')}.0`;
  }
}

const config: NextConfig = {
  output: 'export',
  distDir: process.env.NODE_ENV === 'production' ? '../app' : '.next',
  trailingSlash: true,
  images: {
    unoptimized: true,
  },
  env: {
    OMNI_VERSION: getGitDateVersion(),
  },
};

export default config;
