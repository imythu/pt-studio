import { describe, expect, it } from 'vitest';
import { sanitize } from '../src/index';

describe('UserContent sanitize', () => {
  it('strips scripts and handlers and hardens links', () => {
    const out = sanitize('<p onclick="x()">hi<script>alert(1)</script><a href="https://e.x">l</a><img src=x onerror=alert(1)><iframe src="//e"></iframe></p>');
    expect(out).not.toMatch(/script|onclick|onerror|iframe/);
    expect(out).toContain('rel="nofollow noopener noreferrer ugc"');
    expect(out).toContain('target="_blank"');
  });
  it('drops javascript: urls', () => {
    expect(sanitize('<a href="javascript:alert(1)">x</a>')).not.toContain('javascript:');
  });
  it('keeps only renderer styles, classes and read-only checkboxes', () => {
    const out = sanitize('<span style="color: #f00; background: url(x)" class="mention evil">a</span><input type="text"><input type="checkbox" checked><div class="center">c</div>');
    expect(out).toContain('color');
    expect(out).not.toMatch(/background|evil|type="text"/);
    expect(out).toMatch(/<input[^>]*type="checkbox"[^>]*disabled/);
    expect(out).toContain('class="center"');
  });
});
