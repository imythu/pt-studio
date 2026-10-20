import { describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import { useState } from 'react';
import { Avatar, CopyButton, Empty, MasterDetail, Pager, Sheet, SkipLink, ToastHost, useToast } from '../src/index';

describe('ui-kit 交互组件', () => {
  it('CopyButton 复制并显示已复制', async () => {
    const onCopied = vi.fn();
    Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
    render(<CopyButton text="hello-passkey" onCopied={onCopied} />);
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: '复制' }));
    });
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('hello-passkey');
    expect(onCopied).toHaveBeenCalled();
    expect(screen.getByText('已复制')).toBeDefined();
  });

  it('SkipLink 指向正文', () => {
    render(<SkipLink targetId="content">跳至主要内容</SkipLink>);
    expect(screen.getByRole('link', { name: '跳至主要内容' }).getAttribute('href')).toBe('#content');
  });

  it('Toast 显示并可关闭', () => {
    function Trigger() {
      const toast = useToast();
      return (
        <button type="button" onClick={() => toast('已保存', 'success')}>
          Show
        </button>
      );
    }
    render(
      <ToastHost>
        <Trigger />
      </ToastHost>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Show' }));
    expect(screen.getByText('已保存')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: '关闭提示' }));
    expect(screen.queryByText('已保存')).toBeNull();
  });

  it('Avatar 依次降级：自定义 → 默认头像 → 首字母', () => {
    const { container } = render(<Avatar src="/bad.png" name="Frieren" />);
    fireEvent.error(container.querySelector('img')!);
    expect(container.querySelector('img')?.getAttribute('src')).toBe('/frieren/frieren-avatar-generated.jpg');
    fireEvent.error(container.querySelector('img')!);
    expect(container.textContent).toBe('F');
  });

  it('Empty 显示插画与文案；占位文案只显示插画', () => {
    const { container, rerender } = render(<Empty>暂无种子</Empty>);
    expect(container.querySelector('img')?.getAttribute('src')).toBe('/frieren/frieren-empty-state-generated.jpg');
    expect(container.textContent).toContain('暂无种子');
    rerender(<Empty>—</Empty>);
    expect(container.textContent).toBe('');
  });

  it('Sheet 挂到 body，Esc 关闭', () => {
    function Host() {
      const [open, setOpen] = useState(true);
      return (
        <div data-testid="host">
          <Sheet open={open} onClose={() => setOpen(false)} title="转账">
            <input aria-label="数量" />
          </Sheet>
        </div>
      );
    }
    render(<Host />);
    const dlg = screen.getByRole('dialog', { name: '转账' });
    expect(screen.getByTestId('host').contains(dlg)).toBe(false);
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('MasterDetail 标记当前层级，返回回到列表', () => {
    const onBack = vi.fn();
    const { container } = render(<MasterDetail list={<p>列表</p>} detail={<p>详情</p>} showDetail onBack={onBack} backLabel="收件箱" />);
    expect(container.querySelector('.md')?.getAttribute('data-md')).toBe('detail');
    fireEvent.click(screen.getByRole('button', { name: '‹ 收件箱' }));
    expect(onBack).toHaveBeenCalled();
  });

  it('Pager 折叠中间页码', () => {
    const onChange = vi.fn();
    render(<Pager page={5} pages={20} onChange={onChange} />);
    expect(screen.getByRole('button', { name: '5' }).getAttribute('aria-current')).toBe('page');
    expect(screen.queryByRole('button', { name: '10' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: '下一页' }));
    expect(onChange).toHaveBeenCalledWith(6);
  });
});

describe('Sheet 焦点', () => {
  it('父组件重渲染时焦点留在正在输入的字段', async () => {
    const mm = window.matchMedia;
    window.matchMedia = ((q: string) => ({ matches: true, media: q, addEventListener() {}, removeEventListener() {} })) as unknown as typeof window.matchMedia;
    function Form() {
      const [a, setA] = useState('');
      const [b, setB] = useState('');
      return (
        <Sheet open onClose={() => undefined} title="t">
          <input aria-label="a" value={a} onChange={(e) => setA(e.target.value)} />
          <input aria-label="b" value={b} onChange={(e) => setB(e.target.value)} />
        </Sheet>
      );
    }
    render(<Form />);
    const frame = () => act(() => new Promise((r) => requestAnimationFrame(() => r(undefined))));
    await frame();
    const b = screen.getByLabelText('b') as HTMLInputElement;
    b.focus();
    fireEvent.change(b, { target: { value: 'xyz' } });
    await frame();
    expect(document.activeElement).toBe(b);
    expect(b.value).toBe('xyz');
    window.matchMedia = mm;
  });
});
