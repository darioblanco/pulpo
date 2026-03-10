import { describe, it, expect } from 'vitest';
import { createCamera, worldToScreen, screenToWorld, fitCamera } from './camera';

describe('camera', () => {
  describe('createCamera', () => {
    it('creates camera with given dimensions', () => {
      const cam = createCamera(800, 600);
      expect(cam.width).toBe(800);
      expect(cam.height).toBe(600);
      expect(cam.zoom).toBe(2);
    });
  });

  describe('worldToScreen', () => {
    it('maps world origin to screen center when camera is at origin', () => {
      const cam = createCamera(800, 600);
      cam.x = 0;
      cam.y = 0;
      const [sx, sy] = worldToScreen(cam, 0, 0);
      expect(sx).toBe(400);
      expect(sy).toBe(300);
    });

    it('applies zoom', () => {
      const cam = createCamera(800, 600);
      cam.x = 0;
      cam.y = 0;
      cam.zoom = 3;
      const [sx] = worldToScreen(cam, 10, 0);
      expect(sx).toBe(400 + 10 * 3);
    });

    it('offsets by camera position', () => {
      const cam = createCamera(800, 600);
      cam.x = 50;
      cam.y = 0;
      cam.zoom = 1;
      const [sx] = worldToScreen(cam, 50, 0);
      expect(sx).toBe(400); // world 50 is at center when cam.x=50
    });
  });

  describe('screenToWorld', () => {
    it('is inverse of worldToScreen', () => {
      const cam = createCamera(800, 600);
      cam.x = 100;
      cam.y = 50;
      cam.zoom = 2.5;

      const wx = 75;
      const wy = 120;
      const [sx, sy] = worldToScreen(cam, wx, wy);
      const [rx, ry] = screenToWorld(cam, sx, sy);
      expect(rx).toBeCloseTo(wx, 5);
      expect(ry).toBeCloseTo(wy, 5);
    });

    it('maps screen center to camera position', () => {
      const cam = createCamera(800, 600);
      cam.x = 42;
      cam.y = 99;
      const [wx, wy] = screenToWorld(cam, 400, 300);
      expect(wx).toBeCloseTo(42, 5);
      expect(wy).toBeCloseTo(99, 5);
    });
  });

  describe('fitCamera', () => {
    it('centers camera on single node', () => {
      const cam = createCamera(800, 600);
      fitCamera(cam, [{ x: 0 }]);
      expect(cam.x).toBe(0);
    });

    it('centers camera between two nodes', () => {
      const cam = createCamera(800, 600);
      fitCamera(cam, [{ x: 0 }, { x: 200 }]);
      expect(cam.x).toBe(100);
    });

    it('calculates zoom to fit all nodes', () => {
      const cam = createCamera(800, 600);
      fitCamera(cam, [{ x: 0 }, { x: 400 }], 100);
      // worldWidth = 600 (400 + 2*100), zoomX = 800/600 ≈ 1.33
      // worldHeight = 280, zoomY = 600/280 ≈ 2.14
      // zoom = min(1.33, 2.14, 4) ≈ 1.33
      expect(cam.zoom).toBeCloseTo(800 / 600, 1);
    });

    it('caps zoom at 4', () => {
      const cam = createCamera(4000, 3000);
      fitCamera(cam, [{ x: 0 }], 100);
      expect(cam.zoom).toBeLessThanOrEqual(4);
    });

    it('does nothing with empty nodes', () => {
      const cam = createCamera(800, 600);
      const originalX = cam.x;
      fitCamera(cam, []);
      expect(cam.x).toBe(originalX);
    });
  });
});
