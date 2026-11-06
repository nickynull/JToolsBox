import { exec } from 'child_process';

function run(cmd) {
  return new Promise((resolve, reject) => {
    const child = exec(cmd, (err, stdout, stderr) => {
      if (stdout) process.stdout.write(stdout);
      if (stderr) process.stderr.write(stderr);
      if (err) reject(err);
      else resolve();
    });
    child.on('close', (code) => {
      if (code === 0) resolve();
      else reject(new Error(`${cmd} exited with code ${code}`));
    });
  });
}

await Promise.all([
  run('npm run build'),
  run('npm run build:web:file-sharing'),
]);
