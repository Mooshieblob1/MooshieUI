/** Scene adapted from Aitha camera v1.0.1 (MIT); see public/licenses/anima-camera-LICENSE.txt. */
import * as THREE from "three";
import type { CameraPosition } from "../../utils/animaCamera/logic.js";
import type { CameraAxis } from "../../utils/animaCamera.js";

export function createScene(
  viewport: HTMLElement,
  getState: () => CameraPosition,
  setValue: (key: CameraAxis, value: number) => void,
  commit: () => void,
  unavailable: () => void,
) {
  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  viewport.append(renderer.domElement);
  const canvas = renderer.domElement;
  canvas.className = "block h-full w-full touch-none";
  canvas.setAttribute("aria-hidden", "true");
  const scene = new THREE.Scene(); scene.background = new THREE.Color(0x10101a);
  const camera = new THREE.PerspectiveCamera(45, 1.2, 0.1, 100);
  camera.position.set(4, 3, 4); camera.lookAt(0, 0.5, 0);
  const center = new THREE.Vector3(0, 0.5, 0), radius = 1.8, elevationRadius = 1.4;
  scene.add(new THREE.AmbientLight(0xffffff, 1.2));
  const light = new THREE.DirectionalLight(0xffffff, 2); light.position.set(4, 7, 5); scene.add(light);
  scene.add(new THREE.GridHelper(5, 20, 0x38384b, 0x242434));
  function material(color: number) { return new THREE.MeshStandardMaterial({ color, emissive: color, emissiveIntensity: 0.2 }); }
  function handle(color: number) { const result = new THREE.Mesh(new THREE.SphereGeometry(0.14, 20, 16), material(color)); scene.add(result); return result; }
  const horizontal = handle(0xe95b98), vertical = handle(0x46dfcb), distance = handle(0xffc54d);
  const ring = new THREE.Mesh(new THREE.TorusGeometry(radius, 0.025, 8, 80), new THREE.MeshBasicMaterial({ color: 0xe95b98 }));
  ring.rotation.x = Math.PI / 2; scene.add(ring);
  const points = [];
  for (let i = 0; i <= 40; i++) { const angle = (-30 + 90 * i / 40) * Math.PI / 180; points.push(new THREE.Vector3(-0.8, center.y + elevationRadius * Math.sin(angle), elevationRadius * Math.cos(angle))); }
  scene.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints(points), new THREE.LineBasicMaterial({ color: 0x46dfcb })));
  const card = new THREE.Mesh(new THREE.BoxGeometry(1.1, 1.3, 0.04), material(0x5c577e)); card.position.copy(center); scene.add(card);
  // A simple face establishes the subject's front without downloading assets.
  const face = new THREE.Mesh(new THREE.CircleGeometry(0.17, 24), new THREE.MeshBasicMaterial({ color: 0xe9dbea })); face.position.set(0, 0.76, 0.025); scene.add(face);
  const body = new THREE.Mesh(new THREE.CircleGeometry(0.3, 24), new THREE.MeshBasicMaterial({ color: 0xb39ee0 })); body.scale.set(1, 1.3, 1); body.position.set(0, 0.24, 0.025); scene.add(body);
  const indicator = new THREE.Mesh(new THREE.ConeGeometry(0.13, 0.32, 4), material(0xe95b98)); scene.add(indicator);
  const lineGeometry = new THREE.BufferGeometry().setFromPoints([center, new THREE.Vector3()]);
  const line = new THREE.Line(lineGeometry, new THREE.LineBasicMaterial({ color: 0xffc54d })); scene.add(line);
  function render() { renderer.render(scene, camera); }
  function update(state: CameraPosition) {
    const azimuth = state.horizontal_angle * Math.PI / 180, elevation = state.vertical_angle * Math.PI / 180;
    const extent = 2.6 - state.zoom / 10 * 2;
    const target = new THREE.Vector3(extent * Math.sin(azimuth) * Math.cos(elevation), center.y + extent * Math.sin(elevation), extent * Math.cos(azimuth) * Math.cos(elevation));
    indicator.position.copy(target); indicator.lookAt(center); indicator.rotateX(Math.PI / 2); indicator.rotateY(state.roll * Math.PI / 6);
    horizontal.position.set(radius * Math.sin(azimuth), 0.12, radius * Math.cos(azimuth));
    vertical.position.set(-0.8, center.y + elevationRadius * Math.sin(elevation), elevationRadius * Math.cos(elevation));
    distance.position.lerpVectors(center, target, 0.6);
    const positions = lineGeometry.attributes.position;
    positions.setXYZ(0, center.x, center.y, center.z); positions.setXYZ(1, target.x, target.y, target.z); positions.needsUpdate = true;
    lineGeometry.computeBoundingSphere(); render();
  }
  const resize = new ResizeObserver(() => {
    const width = Math.max(1, viewport.clientWidth), height = Math.max(1, viewport.clientHeight);
    renderer.setSize(width, height, false); camera.aspect = width / height; camera.updateProjectionMatrix(); render();
  });
  resize.observe(viewport);
  const raycaster = new THREE.Raycaster(), pointer = new THREE.Vector2(), handles = [horizontal, vertical, distance];
  let dragging: THREE.Mesh | null = null, lastY = 0;
  const events = new AbortController();
  function ray(event: PointerEvent) {
    const bounds = canvas.getBoundingClientRect();
    pointer.set((event.clientX - bounds.left) / bounds.width * 2 - 1, -(event.clientY - bounds.top) / bounds.height * 2 + 1);
    raycaster.setFromCamera(pointer, camera);
  }
  canvas.addEventListener("pointerdown", (event) => {
    ray(event);
    const hits = raycaster.intersectObjects(handles);
    if (!hits.length) return;
    dragging = hits[0].object as THREE.Mesh; lastY = event.clientY; canvas.setPointerCapture(event.pointerId); event.preventDefault(); event.stopPropagation();
  }, { signal: events.signal });
  canvas.addEventListener("pointermove", (event) => {
    if (!dragging) return;
    ray(event);
    const point = new THREE.Vector3(), plane = new THREE.Plane();
    if (dragging === horizontal) {
      plane.setFromNormalAndCoplanarPoint(new THREE.Vector3(0, 1, 0), new THREE.Vector3());
      if (raycaster.ray.intersectPlane(plane, point)) setValue("horizontal_angle", (Math.atan2(point.x, point.z) * 180 / Math.PI + 360) % 360);
    } else if (dragging === vertical) {
      plane.setFromNormalAndCoplanarPoint(new THREE.Vector3(1, 0, 0), new THREE.Vector3(-0.8, 0, 0));
      if (raycaster.ray.intersectPlane(plane, point)) setValue("vertical_angle", Math.max(-30, Math.min(60, Math.atan2(point.y - center.y, point.z) * 180 / Math.PI)));
    } else setValue("zoom", Math.max(0, Math.min(10, getState().zoom - (event.clientY - lastY) * 0.05)));
    lastY = event.clientY; event.preventDefault(); event.stopPropagation();
  }, { signal: events.signal });
  const release = () => { if (dragging) commit(); dragging = null; };
  for (const event of ["pointerup", "pointercancel", "lostpointercapture"]) canvas.addEventListener(event, release, { signal: events.signal });
  canvas.addEventListener("webglcontextlost", (event) => { event.preventDefault(); unavailable(); }, { signal: events.signal });
  canvas.addEventListener("wheel", (event) => {
    if (!event.composedPath().includes(canvas) || !event.deltaY) return;
    event.preventDefault(); event.stopPropagation();
    setValue("zoom", Math.max(0, Math.min(10, getState().zoom + (event.deltaY < 0 ? 0.25 : -0.25))));
    commit();
  }, { passive: false, signal: events.signal });
  update(getState());
  return {
    update,
    dispose() {
      release(); events.abort(); resize.disconnect();
      scene.traverse((object) => {
        if (object instanceof THREE.Mesh || object instanceof THREE.Line) {
          object.geometry.dispose();
          const materials = Array.isArray(object.material) ? object.material : [object.material];
          for (const item of materials) item.dispose();
        }
      });
      renderer.dispose(); renderer.forceContextLoss(); canvas.remove();
    },
  };
}
