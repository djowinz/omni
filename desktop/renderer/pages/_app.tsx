import { useEffect } from 'react';
import type { AppProps } from 'next/app';
import '../styles/globals.css';

export default function App({ Component, pageProps }: AppProps) {
  // Load feather icon font at runtime via omni:// protocol.
  // This can't be done in CSS files because PostCSS processes them at build time
  // and can't resolve the custom protocol.
  useEffect(() => {
    const style = document.createElement('style');
    style.textContent = `
      @font-face {
        font-family: "feather";
        src: url("omni://resource/feather.ttf") format("truetype");
        font-weight: normal;
        font-style: normal;
      }
      @font-face {
        font-family: "icomoon";
        src: url("omni://resource/feather.ttf") format("truetype");
        font-weight: normal;
        font-style: normal;
      }
    `;
    document.head.appendChild(style);
    return () => { document.head.removeChild(style); };
  }, []);

  return (
    <div className="dark">
      <Component {...pageProps} />
    </div>
  );
}
