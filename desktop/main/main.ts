import path from 'path';
import { app, BrowserWindow } from 'electron';

const isProd = process.env.NODE_ENV === 'production';

function createWindow() {
  const win = new BrowserWindow({
    width: 900,
    height: 680,
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
    },
  });

  if (isProd) {
    win.loadFile(path.join(__dirname, '../app/home/index.html'));
  } else {
    const port = process.argv[2] || '8888';
    win.loadURL(`http://localhost:${port}/home`);
  }
}

app.whenReady().then(createWindow);

app.on('window-all-closed', () => {
  app.quit();
});
