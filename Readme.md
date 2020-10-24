![logo](docs/images/visioncortex-banner.png)

# VTracer

VTracer is a program to convert raster images (like jpg & png) into vector graphics (svg). It can vectorize graphics and photographs and trace the curves to output compact vector files.

Comparing to Potrace which only accept binarized inputs (Black & White pixmap), VTracer has an image processing pipeline which can handle colored inputs. 

Comparing to Adobe Illustrator's Live Trace, VTracer's output is much more compact (less curves) as we adopt a stacking strategy and avoid producing shapes with holes.

A detailed description of the algorithm is at [visioncortex.org/vtracer-docs](//www.visioncortex.org/vtracer-docs).

![screenshot](docs/images/screenshot-01.png)

![screenshot](docs/images/screenshot-02.png)