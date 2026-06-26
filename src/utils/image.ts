/**
 * Compress a picked image file down to a small data URL so it fits comfortably
 * inside localStorage (~5 MB quota) and renders reliably as a CSS
 * `background-image`.
 *
 * Why this exists: background images used to be stored as a raw base64 data
 * URL straight from disk. The backend allows up to 8 MB, but base64 inflates
 * that ~33% and JSON-serializing it pushed many real photos over the
 * localStorage quota — the `setItem` then threw, was silently swallowed, and
 * the image never persisted (looked like "preview works, but doesn't apply /
 * vanishes on restart"). Downsampling to ≤1920px on the long edge and
 * re-encoding as JPEG 0.85 typically lands well under 200 KB, which is safe
 * to store and fast to paint.
 */

const MAX_DIMENSION = 1920;
const JPEG_QUALITY = 0.85;

/** Read a File (or the path-based data URL the backend hands us) into an
 * HTMLImageElement we can draw onto a canvas. Accepts both a real File (from
 * an <input type=file>, if ever used) and a data URL string. */
function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("图片解码失败"));
    img.src = src;
  });
}

/**
 * Compress the given data URL. Returns a JPEG data URL downscaled to fit
 * within MAX_DIMENSION×MAX_DIMENSION (preserving aspect ratio). Falls back to
 * the original src if anything fails — compression is best-effort; a working
 * (if large) image beats no image.
 *
 * @param dataUrl source image as a data URL (e.g. from readFileBase64)
 * @returns compressed JPEG data URL, or the original on failure
 */
export async function compressImageDataUrl(dataUrl: string): Promise<string> {
  try {
    const img = await loadImage(dataUrl);

    // Compute the target size — only downscale, never upscale.
    let { width, height } = img;
    if (width > MAX_DIMENSION || height > MAX_DIMENSION) {
      if (width >= height) {
        height = Math.round((height * MAX_DIMENSION) / width);
        width = MAX_DIMENSION;
      } else {
        width = Math.round((width * MAX_DIMENSION) / height);
        height = MAX_DIMENSION;
      }
    }

    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return dataUrl; // no 2D context — keep original
    ctx.drawImage(img, 0, 0, width, height);

    // JPEG for size; a background image doesn't need alpha (opacity is
    // handled by the container). PNG would be far larger.
    return canvas.toDataURL("image/jpeg", JPEG_QUALITY);
  } catch (e) {
    console.warn("[compressImage] compression failed, using original", e);
    return dataUrl;
  }
}
