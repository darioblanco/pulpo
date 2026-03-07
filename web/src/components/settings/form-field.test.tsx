import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { FormField } from './form-field';

describe('FormField', () => {
  it('renders label and children', () => {
    render(
      <FormField label="Name" htmlFor="name">
        <input id="name" />
      </FormField>,
    );
    expect(screen.getByText('Name')).toBeInTheDocument();
    expect(screen.getByRole('textbox')).toBeInTheDocument();
  });

  it('renders description when provided', () => {
    render(
      <FormField label="Port" description="Must be between 1-65535.">
        <input />
      </FormField>,
    );
    expect(screen.getByText('Must be between 1-65535.')).toBeInTheDocument();
  });

  it('does not render description when omitted', () => {
    const { container } = render(
      <FormField label="Name">
        <input />
      </FormField>,
    );
    expect(container.querySelectorAll('p')).toHaveLength(0);
  });
});
