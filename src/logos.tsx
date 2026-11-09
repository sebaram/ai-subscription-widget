// Small colored provider logos for the float bar / popup headers.
// Recognizable simplified marks; unknown providers fall back to a colored dot.

function ClaudeLogo({ color }: { color: string }) {
  // Anthropic-style 12-ray starburst
  const rays = Array.from({ length: 12 }, (_, i) => (i * 360) / 12);
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden>
      {rays.map((a) => (
        <line
          key={a}
          x1="12" y1="12"
          x2={12 + 9 * Math.cos((a * Math.PI) / 180)}
          y2={12 + 9 * Math.sin((a * Math.PI) / 180)}
          stroke={color}
          strokeWidth="2.6"
          strokeLinecap="round"
        />
      ))}
      <circle cx="12" cy="12" r="2.6" fill={color} />
    </svg>
  );
}

function OpenAILogo({ color }: { color: string }) {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden>
      <path
        fill={color}
        d="M22.2819 9.8211a5.0043 5.0043 0 0 0-4.8149-4.3682 4.9739 4.9739 0 0 0-1.5276-1.9906 5.0262 5.0262 0 0 0-8.1982 2.3017 5.0036 5.0036 0 0 0-4.3682 4.8149 4.9739 4.9739 0 0 0 1.9906 1.5276A5.0262 5.0262 0 0 0 7.5642 21.924a5.0055 5.0055 0 0 0 5.4177-2.3015 5.0036 5.0036 0 0 0 4.3682-4.8149 4.9739 4.9739 0 0 0-1.9906-1.5276zM12.16 8.7654l3.1063 1.7942v3.5883L12.16 15.9421l-3.1063-1.7942v-3.5883z"
      />
    </svg>
  );
}

function GoogleLogo() {
  // 4-color G
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden>
      <path fill="#4285F4" d="M23.49 12.27c0-.79-.07-1.54-.19-2.27H12v4.51h6.47a5.57 5.57 0 0 1-2.4 3.58v3h3.86c2.26-2.09 3.56-5.17 3.56-8.82z" />
      <path fill="#34A853" d="M12 24c3.24 0 5.95-1.08 7.93-2.91l-3.86-3c-1.08.72-2.45 1.16-4.07 1.16-3.13 0-5.78-2.11-6.73-4.96H1.29v3.09C3.26 21.3 7.31 24 12 24z" />
      <path fill="#FBBC05" d="M5.27 14.29A7.2 7.2 0 0 1 4.89 12c0-.8.14-1.57.38-2.29V6.62H1.29A12 12 0 0 0 0 12c0 1.94.46 3.77 1.29 5.38l3.98-3.09z" />
      <path fill="#EA4335" d="M12 4.75c1.77 0 3.35.61 4.6 1.8l3.42-3.42C17.95 1.19 15.24 0 12 0 7.31 0 3.26 2.7 1.29 6.62l3.98 3.09C6.22 6.86 8.87 4.75 12 4.75z" />
    </svg>
  );
}

function OllamaLogo({ color }: { color: string }) {
  // simplified llama head with ears
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden>
      <path
        fill={color}
        d="M9 2.6c-1 0-2 1.1-2 2.6L6.6 7.9C5.4 9.1 4.6 10.8 4.6 12.7c0 4.4 3.3 7.9 7.4 7.9s7.4-3.5 7.4-7.9c0-1.9-.8-3.6-2-4.8L17 5.2c0-1.5-1-2.6-2-2.6-1 0-1.7 1-1.9 2.4a9.2 9.2 0 0 0-2.2 0C10.7 3.6 10 2.6 9 2.6z"
      />
      <circle cx="9.4" cy="12.4" r="1" fill="#171c26" />
      <circle cx="14.6" cy="12.4" r="1" fill="#171c26" />
    </svg>
  );
}

export function ProviderLogo({ id, color }: { id: string; color: string }) {
  switch (id) {
    case 'claude':
      return <ClaudeLogo color={color} />;
    case 'codex':
      return <OpenAILogo color={color} />;
    case 'antigravity':
      return <GoogleLogo />;
    case 'ollama':
      return <OllamaLogo color={color} />;
    default:
      return <span style={{ width: 14, height: 14, borderRadius: '50%', background: color, display: 'inline-block' }} />;
  }
}
