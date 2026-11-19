import type { Meta, StoryObj } from '@storybook/react-vite';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function Label({ children }: { children: React.ReactNode }) {
  return (
    <span style={{
      fontFamily: 'var(--font-mono)',
      fontSize: '11px',
      color: 'var(--text-2)',
    }}>
      {children}
    </span>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h3 style={{
      fontFamily: 'var(--font-ui)',
      fontSize: 'var(--text-md)',
      fontWeight: 'var(--font-semibold)' as React.CSSProperties['fontWeight'],
      color: 'var(--text-0)',
      margin: '0 0 16px 0',
      paddingBottom: '8px',
      borderBottom: '1px solid var(--border-1)',
    }}>
      {children}
    </h3>
  );
}

function GroupTitle({ children }: { children: React.ReactNode }) {
  return (
    <p style={{
      fontFamily: 'var(--font-ui)',
      fontSize: 'var(--text-sm)',
      fontWeight: 'var(--font-medium)' as React.CSSProperties['fontWeight'],
      color: 'var(--text-1)',
      margin: '16px 0 8px 0',
      textTransform: 'uppercase' as const,
      letterSpacing: '0.05em',
    }}>
      {children}
    </p>
  );
}

// ---------------------------------------------------------------------------
// Color swatches
// ---------------------------------------------------------------------------

interface SwatchProps {
  token: string;
  value: string;
}

function Swatch({ token, value }: SwatchProps) {
  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      gap: '6px',
      minWidth: '96px',
    }}>
      <div style={{
        width: '100%',
        height: '56px',
        background: `var(${token})`,
        borderRadius: 'var(--radius-sm)',
        border: '1px solid var(--border-1)',
      }} />
      <Label>{token}</Label>
      <span style={{
        fontFamily: 'var(--font-mono)',
        fontSize: '10px',
        color: 'var(--text-3)',
      }}>
        {value}
      </span>
    </div>
  );
}

function ColorRow({ tokens }: { tokens: Array<{ token: string; value: string }> }) {
  return (
    <div style={{ display: 'flex', flexWrap: 'wrap', gap: '16px', marginBottom: '8px' }}>
      {tokens.map(({ token, value }) => (
        <Swatch key={token} token={token} value={value} />
      ))}
    </div>
  );
}

function ColorsStory() {
  return (
    <div style={{ padding: '32px', background: 'var(--bg-1)', minHeight: '100vh' }}>
      <SectionTitle>Colors</SectionTitle>

      <GroupTitle>Backgrounds</GroupTitle>
      <ColorRow tokens={[
        { token: '--bg-0', value: '#0b0e11' },
        { token: '--bg-1', value: '#111418' },
        { token: '--bg-2', value: '#161a1e' },
        { token: '--bg-3', value: '#1e2329' },
        { token: '--bg-4', value: '#252a30' },
      ]} />

      <GroupTitle>Text</GroupTitle>
      <ColorRow tokens={[
        { token: '--text-0', value: '#eaecef' },
        { token: '--text-1', value: '#b7bdc6' },
        { token: '--text-2', value: '#848e9c' },
        { token: '--text-3', value: '#5e6673' },
      ]} />

      <GroupTitle>Borders</GroupTitle>
      <ColorRow tokens={[
        { token: '--border-0', value: 'rgba(255,255,255,0.04)' },
        { token: '--border-1', value: 'rgba(255,255,255,0.08)' },
        { token: '--border-2', value: 'rgba(255,255,255,0.12)' },
      ]} />

      <GroupTitle>Long (Buy)</GroupTitle>
      <ColorRow tokens={[
        { token: '--long', value: '#0ecb81' },
        { token: '--long-bg', value: 'rgba(14,203,129,0.10)' },
        { token: '--long-muted', value: 'rgba(14,203,129,0.18)' },
        { token: '--long-hover', value: '#09a86a' },
      ]} />

      <GroupTitle>Short (Sell)</GroupTitle>
      <ColorRow tokens={[
        { token: '--short', value: '#f6465d' },
        { token: '--short-bg', value: 'rgba(246,70,93,0.10)' },
        { token: '--short-muted', value: 'rgba(246,70,93,0.18)' },
        { token: '--short-hover', value: '#d93752' },
      ]} />

      <GroupTitle>Accent</GroupTitle>
      <ColorRow tokens={[
        { token: '--accent', value: '#2ee5a6' },
        { token: '--accent-bg', value: 'rgba(46,229,166,0.12)' },
        { token: '--accent-muted', value: 'rgba(46,229,166,0.20)' },
        { token: '--accent-hover', value: '#26c98f' },
      ]} />

      <GroupTitle>Semantic</GroupTitle>
      <ColorRow tokens={[
        { token: '--info', value: '#3b82f6' },
        { token: '--info-bg', value: 'rgba(59,130,246,0.10)' },
        { token: '--warning', value: '#f0b90b' },
        { token: '--warning-bg', value: 'rgba(240,185,11,0.10)' },
        { token: '--error', value: 'var(--short)' },
        { token: '--error-bg', value: 'var(--short-bg)' },
        { token: '--success', value: 'var(--long)' },
        { token: '--success-bg', value: 'var(--long-bg)' },
      ]} />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Typography
// ---------------------------------------------------------------------------

function TypographyStory() {
  const sizes: Array<{ token: string; label: string; value: string }> = [
    { token: '--text-xs',   label: 'xs',   value: '11px' },
    { token: '--text-sm',   label: 'sm',   value: '12px' },
    { token: '--text-base', label: 'base', value: '13px' },
    { token: '--text-md',   label: 'md',   value: '14px' },
    { token: '--text-lg',   label: 'lg',   value: '16px' },
    { token: '--text-xl',   label: 'xl',   value: '20px' },
  ];

  const weights: Array<{ token: string; label: string; value: string }> = [
    { token: '--font-normal',   label: 'normal',   value: '400' },
    { token: '--font-medium',   label: 'medium',   value: '500' },
    { token: '--font-semibold', label: 'semibold', value: '600' },
    { token: '--font-bold',     label: 'bold',     value: '700' },
  ];

  const families: Array<{ token: string; label: string; description: string }> = [
    { token: '--font-sans', label: 'Sans (UI headings)', description: 'Plus Jakarta Sans, Inter' },
    { token: '--font-data', label: 'Data (numbers)',     description: 'DM Sans, Inter' },
    { token: '--font-mono', label: 'Mono (code / hash)', description: 'JetBrains Mono, SF Mono' },
  ];

  return (
    <div style={{ padding: '32px', background: 'var(--bg-1)', minHeight: '100vh' }}>
      <SectionTitle>Typography</SectionTitle>

      <GroupTitle>Font Sizes</GroupTitle>
      <div style={{ display: 'flex', flexDirection: 'column', gap: '12px', marginBottom: '24px' }}>
        {sizes.map(({ token, value }) => (
          <div key={token} style={{ display: 'flex', alignItems: 'baseline', gap: '16px' }}>
            <div style={{ width: '160px', flexShrink: 0 }}>
              <Label>{token}</Label>
              <span style={{ color: 'var(--text-3)', fontFamily: 'var(--font-mono)', fontSize: '10px', marginLeft: '8px' }}>
                {value}
              </span>
            </div>
            <span style={{
              fontFamily: 'var(--font-ui)',
              fontSize: `var(${token})`,
              color: 'var(--text-0)',
            }}>
              BTCUSDT — 68,420.50 (+2.34%)
            </span>
          </div>
        ))}
      </div>

      <GroupTitle>Font Weights</GroupTitle>
      <div style={{ display: 'flex', flexDirection: 'column', gap: '12px', marginBottom: '24px' }}>
        {weights.map(({ token, label, value }) => (
          <div key={token} style={{ display: 'flex', alignItems: 'baseline', gap: '16px' }}>
            <div style={{ width: '160px', flexShrink: 0 }}>
              <Label>{token}</Label>
              <span style={{ color: 'var(--text-3)', fontFamily: 'var(--font-mono)', fontSize: '10px', marginLeft: '8px' }}>
                {value}
              </span>
            </div>
            <span style={{
              fontFamily: 'var(--font-ui)',
              fontSize: 'var(--text-lg)',
              fontWeight: `var(${token})` as React.CSSProperties['fontWeight'],
              color: 'var(--text-0)',
            }}>
              {label.charAt(0).toUpperCase() + label.slice(1)} weight sample
            </span>
          </div>
        ))}
      </div>

      <GroupTitle>Font Families</GroupTitle>
      <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
        {families.map(({ token, label, description }) => (
          <div key={token} style={{
            padding: '16px',
            background: 'var(--bg-2)',
            borderRadius: 'var(--radius)',
            border: '1px solid var(--border-1)',
          }}>
            <div style={{ marginBottom: '4px' }}>
              <Label>{token}</Label>
              <span style={{ color: 'var(--text-3)', fontFamily: 'var(--font-mono)', fontSize: '10px', marginLeft: '8px' }}>
                {description}
              </span>
            </div>
            <span style={{
              fontFamily: `var(${token})`,
              fontSize: 'var(--text-lg)',
              color: 'var(--text-0)',
            }}>
              {label}: The quick brown fox — 0.00123456 BTC
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Spacing
// ---------------------------------------------------------------------------

function SpacingStory() {
  const spacings: Array<{ token: string; value: string }> = [
    { token: '--pad-2xs', value: '2px' },
    { token: '--pad-xs',  value: '4px' },
    { token: '--pad-sm',  value: '8px' },
    { token: '--pad',     value: '12px' },
    { token: '--pad-lg',  value: '16px' },
    { token: '--pad-xl',  value: '24px' },
    { token: '--pad-xxl', value: '32px' },
  ];

  const layoutTokens: Array<{ token: string; value: string; description: string }> = [
    { token: '--gap',         value: '1px',  description: 'Row separator gap' },
    { token: '--header-h',    value: '48px', description: 'Top navigation height' },
    { token: '--sidebar-w',   value: '240px', description: 'Left sidebar width' },
    { token: '--statusbar-h', value: '24px', description: 'Bottom status bar height' },
  ];

  return (
    <div style={{ padding: '32px', background: 'var(--bg-1)', minHeight: '100vh' }}>
      <SectionTitle>Spacing</SectionTitle>

      <GroupTitle>Padding Scale</GroupTitle>
      <div style={{ display: 'flex', flexDirection: 'column', gap: '12px', marginBottom: '32px' }}>
        {spacings.map(({ token, value }) => (
          <div key={token} style={{ display: 'flex', alignItems: 'center', gap: '16px' }}>
            <div style={{ width: '120px', flexShrink: 0 }}>
              <Label>{token}</Label>
            </div>
            <span style={{ color: 'var(--text-3)', fontFamily: 'var(--font-mono)', fontSize: '10px', width: '36px', flexShrink: 0 }}>
              {value}
            </span>
            <div style={{
              height: '20px',
              width: `var(${token})`,
              minWidth: '2px',
              background: 'var(--accent)',
              borderRadius: '2px',
              transition: 'width var(--transition-normal)',
            }} />
            <div style={{
              height: '20px',
              flex: 1,
              background: 'var(--bg-3)',
              borderRadius: '2px',
            }} />
          </div>
        ))}
      </div>

      <GroupTitle>Layout Tokens</GroupTitle>
      <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
        {layoutTokens.map(({ token, value, description }) => (
          <div key={token} style={{
            display: 'flex',
            alignItems: 'center',
            gap: '16px',
            padding: '10px 14px',
            background: 'var(--bg-2)',
            borderRadius: 'var(--radius-sm)',
            border: '1px solid var(--border-1)',
          }}>
            <Label>{token}</Label>
            <span style={{ color: 'var(--accent)', fontFamily: 'var(--font-mono)', fontSize: '11px', minWidth: '48px' }}>
              {value}
            </span>
            <span style={{ color: 'var(--text-2)', fontFamily: 'var(--font-ui)', fontSize: 'var(--text-sm)' }}>
              {description}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Borders & Shadows
// ---------------------------------------------------------------------------

function BordersStory() {
  const radii: Array<{ token: string; value: string }> = [
    { token: '--radius-sm',   value: '6px' },
    { token: '--radius',      value: '10px' },
    { token: '--radius-lg',   value: '14px' },
    { token: '--radius-xl',   value: '20px' },
    { token: '--radius-full', value: '9999px' },
  ];

  const shadows: Array<{ token: string; description: string }> = [
    { token: '--shadow-sm',      description: 'Subtle depth — table rows, inputs' },
    { token: '--shadow-md',      description: 'Panels, cards, dropdowns' },
    { token: '--shadow-lg',      description: 'Modals, popovers' },
    { token: '--shadow-overlay', description: 'Full-screen overlays' },
    { token: '--shadow-glow',    description: 'Accent glow — active state highlight' },
  ];

  return (
    <div style={{ padding: '32px', background: 'var(--bg-1)', minHeight: '100vh' }}>
      <SectionTitle>Borders &amp; Shadows</SectionTitle>

      <GroupTitle>Border Radius</GroupTitle>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '24px', marginBottom: '32px' }}>
        {radii.map(({ token, value }) => (
          <div key={token} style={{ display: 'flex', flexDirection: 'column', gap: '8px', alignItems: 'center' }}>
            <div style={{
              width: '80px',
              height: '80px',
              background: 'var(--bg-3)',
              border: '2px solid var(--accent)',
              borderRadius: `var(${token})`,
            }} />
            <Label>{token}</Label>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: '10px', color: 'var(--text-3)' }}>
              {value}
            </span>
          </div>
        ))}
      </div>

      <GroupTitle>Shadows</GroupTitle>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '24px' }}>
        {shadows.map(({ token, description }) => (
          <div key={token} style={{ display: 'flex', flexDirection: 'column', gap: '10px', width: '180px' }}>
            <div style={{
              height: '80px',
              background: 'var(--bg-3)',
              borderRadius: 'var(--radius)',
              boxShadow: `var(${token})`,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}>
              <span style={{ color: 'var(--text-2)', fontFamily: 'var(--font-mono)', fontSize: '10px' }}>
                {token.replace('--shadow-', '')}
              </span>
            </div>
            <Label>{token}</Label>
            <span style={{ fontFamily: 'var(--font-ui)', fontSize: '11px', color: 'var(--text-3)', lineHeight: 1.4 }}>
              {description}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Meta & story exports
// ---------------------------------------------------------------------------

const meta: Meta = {
  title: 'Design System/Tokens',
  parameters: {
    layout: 'fullscreen',
    options: { showPanel: false },
  },
};

export default meta;

export const Colors: StoryObj = {
  render: () => <ColorsStory />,
};

export const Typography: StoryObj = {
  render: () => <TypographyStory />,
};

export const Spacing: StoryObj = {
  render: () => <SpacingStory />,
};

export const Borders: StoryObj = {
  render: () => <BordersStory />,
};
