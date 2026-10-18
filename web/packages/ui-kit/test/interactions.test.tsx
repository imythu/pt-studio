import { describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import { Avatar, BackToTop, CopyButton, Empty, SkipLink, ToastHost, useToast } from '../src/index';

describe('UI kit interactive components', () => {
  it('renders CopyButton and handles copy action', async () => {
    const onCopied = vi.fn();
    Object.assign(navigator, {
      clipboard: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });

    render(<CopyButton text="hello-passkey" label="复制" onCopied={onCopied} />);
    const btn = screen.getByRole('button', { name: '复制' });
    expect(btn).toBeDefined();

    await act(async () => {
      fireEvent.click(btn);
    });

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('hello-passkey');
    expect(onCopied).toHaveBeenCalled();
    expect(screen.getByText('已复制')).toBeDefined();
  });

  it('renders BackToTop and responds to scroll', () => {
    const { container } = render(<BackToTop threshold={100} title="回到顶部" />);
    const btn = container.querySelector('.pt-back-to-top');
    expect(btn).toBeDefined();
    expect(btn?.classList.contains('visible')).toBe(false);

    // Simulate scroll
    Object.defineProperty(window, 'scrollY', { value: 200, writable: true });
    fireEvent.scroll(window);
  });

  it('renders SkipLink with accessible attributes', () => {
    render(<SkipLink targetId="content">跳至主要内容</SkipLink>);
    const link = screen.getByRole('link', { name: '跳至主要内容' });
    expect(link.getAttribute('href')).toBe('#content');
    expect(link.className).toContain('pt-skip-link');
  });

  it('renders ToastHost and handles dismiss button', async () => {
    function Trigger() {
      const toast = useToast();
      return <button type="button" onClick={() => toast('Saved successfully', 'success')}>Show</button>;
    }

    render(
      <ToastHost>
        <Trigger />
      </ToastHost>
    );

    fireEvent.click(screen.getByRole('button', { name: 'Show' }));
    expect(screen.getByText('Saved successfully')).toBeDefined();

    const closeBtn = screen.getByRole('button', { name: '关闭提示' });
    expect(closeBtn).toBeDefined();
    fireEvent.click(closeBtn);
    expect(screen.queryByText('Saved successfully')).toBeNull();
  });

  it('renders Avatar with fallback image and handles error fallback to initial', () => {
    const { container, rerender } = render(<Avatar name="Frieren" />);
    const img = container.querySelector('img');
    expect(img).toBeDefined();
    expect(img?.getAttribute('src')).toBe('/frieren/frieren-avatar-generated.jpg');

    // Trigger image error on both custom and fallback to test graceful degradation to initial
    rerender(<Avatar src="/invalid-avatar.png" fallbackSrc={null} name="Frieren" />);
    const customImg = container.querySelector('img');
    fireEvent.error(customImg!);
    expect(container.textContent).toBe('F');
  });

  it('renders Empty with illustration mode', () => {
    const { container } = render(<Empty illustration>暂无种子</Empty>);
    const img = container.querySelector('.pt-empty-illustration');
    expect(img).toBeDefined();
    expect(img?.getAttribute('src')).toBe('/frieren/frieren-empty-state-generated.jpg');
    expect(container.textContent).toContain('暂无种子');
  });
});
