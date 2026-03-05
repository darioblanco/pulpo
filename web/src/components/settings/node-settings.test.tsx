import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { NodeSettings } from './node-settings';

describe('NodeSettings', () => {
  it('renders all fields', () => {
    render(
      <NodeSettings
        name="my-node"
        onNameChange={vi.fn()}
        port={7433}
        onPortChange={vi.fn()}
        dataDir="/data"
        onDataDirChange={vi.fn()}
      />,
    );
    expect(screen.getByTestId('node-settings')).toBeInTheDocument();
    expect(screen.getByLabelText('Name')).toHaveValue('my-node');
    expect(screen.getByLabelText('Port')).toHaveValue(7433);
    expect(screen.getByLabelText('Data directory')).toHaveValue('/data');
  });

  it('calls onNameChange', () => {
    const onNameChange = vi.fn();
    render(
      <NodeSettings
        name=""
        onNameChange={onNameChange}
        port={7433}
        onPortChange={vi.fn()}
        dataDir=""
        onDataDirChange={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'new-name' } });
    expect(onNameChange).toHaveBeenCalledWith('new-name');
  });

  it('calls onPortChange with parsed integer', () => {
    const onPortChange = vi.fn();
    render(
      <NodeSettings
        name=""
        onNameChange={vi.fn()}
        port={7433}
        onPortChange={onPortChange}
        dataDir=""
        onDataDirChange={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText('Port'), { target: { value: '8080' } });
    expect(onPortChange).toHaveBeenCalledWith(8080);
  });

  it('calls onPortChange with 0 for invalid input', () => {
    const onPortChange = vi.fn();
    render(
      <NodeSettings
        name=""
        onNameChange={vi.fn()}
        port={7433}
        onPortChange={onPortChange}
        dataDir=""
        onDataDirChange={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText('Port'), { target: { value: 'abc' } });
    expect(onPortChange).toHaveBeenCalledWith(0);
  });

  it('calls onDataDirChange', () => {
    const onDataDirChange = vi.fn();
    render(
      <NodeSettings
        name=""
        onNameChange={vi.fn()}
        port={7433}
        onPortChange={vi.fn()}
        dataDir=""
        onDataDirChange={onDataDirChange}
      />,
    );
    fireEvent.change(screen.getByLabelText('Data directory'), { target: { value: '/new/dir' } });
    expect(onDataDirChange).toHaveBeenCalledWith('/new/dir');
  });
});
