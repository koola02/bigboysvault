#!/usr/bin/env python3
"""Straightens and crops a photo of one trading card, so only the card is left (the table, hand or mat behind it
goes). Usage: cardcrop.py in.jpg out.jpg

Finds the card's four corners (the biggest four-sided shape whose sides are about 5:7), then warps it flat into a
744 x 1039 portrait picture. Exit codes: 0 done, 2 no card found or nothing to improve (the photo stays as it
was), 3 the file couldn't be read. Needs: opencv (headless) and numpy, from ops/setup-extras.sh."""
import sys

import cv2
import numpy as np

CARD = 63 / 88  # a Pokémon card's width over its height
OUT_W, OUT_H = 744, 1039


def order(pts):
    """Top-left, top-right, bottom-right, bottom-left."""
    s = pts.sum(axis=1)
    d = np.diff(pts, axis=1).ravel()
    return np.array([pts[np.argmin(s)], pts[np.argmin(d)], pts[np.argmax(s)], pts[np.argmax(d)]], dtype="float32")


def sides(q):
    w = (np.linalg.norm(q[1] - q[0]) + np.linalg.norm(q[2] - q[3])) / 2
    h = (np.linalg.norm(q[3] - q[0]) + np.linalg.norm(q[2] - q[1])) / 2
    return w, h


def card_like(q):
    w, h = sides(q)
    if min(w, h) <= 0:
        return False
    ratio = min(w, h) / max(w, h)
    return abs(ratio - CARD) < 0.09


def find_card(img):
    h, w = img.shape[:2]
    scale = 1000 / max(h, w) if max(h, w) > 1000 else 1.0
    small = cv2.resize(img, (int(w * scale), int(h * scale))) if scale != 1.0 else img
    area = small.shape[0] * small.shape[1]
    gray = cv2.cvtColor(small, cv2.COLOR_BGR2GRAY)
    gray = cv2.GaussianBlur(gray, (5, 5), 0)
    for lo, hi in ((40, 140), (20, 80), (60, 200)):
        edges = cv2.dilate(cv2.Canny(gray, lo, hi), np.ones((3, 3), np.uint8), iterations=2)
        cnts, _ = cv2.findContours(edges, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
        for c in sorted(cnts, key=cv2.contourArea, reverse=True)[:6]:
            a = cv2.contourArea(c)
            if a < 0.18 * area:
                break
            approx = cv2.approxPolyDP(c, 0.02 * cv2.arcLength(c, True), True)
            if len(approx) == 4 and cv2.isContourConvex(approx):
                q = order(approx.reshape(4, 2).astype("float32"))
            else:
                q = order(cv2.boxPoints(cv2.minAreaRect(c)).astype("float32"))
            if card_like(q):
                return q / scale, a / area
    return None, 0.0


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        return 3
    img = cv2.imread(sys.argv[1], cv2.IMREAD_COLOR)
    if img is None:
        return 3
    # A photo that already has a card's shape is the card itself, cropped: the shapes inside it (the artwork's
    # frame, the text box) are not the card's edges, so it is left alone. A phone photo (3:4) is not this shape.
    ih, iw = img.shape[:2]
    if abs(min(ih, iw) / max(ih, iw) - CARD) < 0.02:
        return 2
    q, cover = find_card(img)
    if q is None:
        return 2
    # Already a straight, tight photo of the card: nothing to improve.
    w, h = sides(q)
    tilt = abs(np.degrees(np.arctan2(q[1][1] - q[0][1], q[1][0] - q[0][0])))
    if cover > 0.9 and tilt < 1.5:
        return 2
    portrait = h >= w
    dst = np.array([[0, 0], [OUT_W - 1, 0], [OUT_W - 1, OUT_H - 1], [0, OUT_H - 1]], dtype="float32")
    if not portrait:  # a card lying on its side comes out standing up
        dst = np.array([[0, OUT_H - 1], [0, 0], [OUT_W - 1, 0], [OUT_W - 1, OUT_H - 1]], dtype="float32")
    m = cv2.getPerspectiveTransform(q, dst)
    out = cv2.warpPerspective(img, m, (OUT_W, OUT_H), flags=cv2.INTER_CUBIC, borderMode=cv2.BORDER_REPLICATE)
    cv2.imwrite(sys.argv[2], out, [cv2.IMWRITE_JPEG_QUALITY, 92])
    return 0


if __name__ == "__main__":
    sys.exit(main())
