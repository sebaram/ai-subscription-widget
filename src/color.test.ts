import { describe, expect, it } from 'vitest';
import { colorFor } from './color';

const th = { amber: 50, red: 80 };

describe('colorFor', () => {
  it('green below amber threshold', () => expect(colorFor(49, th)).toBe('#34c07c'));
  it('amber at amber threshold', () => expect(colorFor(50, th)).toBe('#e2b93d'));
  it('amber below red threshold', () => expect(colorFor(79.9, th)).toBe('#e2b93d'));
  it('red at red threshold', () => expect(colorFor(80, th)).toBe('#e8503a'));
  it('green at zero', () => expect(colorFor(0, th)).toBe('#34c07c'));
});
