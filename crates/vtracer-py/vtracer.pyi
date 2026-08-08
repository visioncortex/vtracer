from typing import Optional

__version__: str

class Config:
    """Conversion configuration. Construct with keyword arguments or a preset,
    mutate via properties, then call one of the ``convert_*`` methods."""

    def __init__(
        self,
        clustering: str = "color-cluster",  # "color-cluster" | "bw" | "watershed"
        hierarchical: str = "stacked",      # "stacked" | "cutout" (mosaic)
        mode: str = "spline",               # "pixel" | "polygon" | "spline"
        filter_speckle: int = 4,
        color_precision: int = 6,
        layer_difference: int = 16,
        corner_threshold: int = 60,
        length_threshold: float = 4.0,
        max_iterations: int = 10,
        splice_threshold: int = 45,
        simplify: Optional[float] = None,   # curve simplification tolerance in px (None = off)
        path_precision: int = 2,
        palette: Optional[list[str]] = None,   # e.g. ["#112233", "#445566"]
        max_colors: Optional[int] = None,      # auto-quantize target
        optimize: int = 1,                     # 0 | 1 | 2
        binary_threshold: int = 128,           # bw: fixed cutoff 0..=255
        adaptive: bool = False,                # bw: Bradley–Roth adaptive
        adaptive_window: int = 0,              # bw adaptive: window px (0 = auto)
        adaptive_t: float = 15.0,              # bw adaptive: % below local mean
        watershed_detail: int = 128,           # watershed: cut level (higher = more regions, uncapped)
    ) -> None: ...

    @staticmethod
    def bw() -> "Config": ...
    @staticmethod
    def poster() -> "Config": ...
    @staticmethod
    def photo() -> "Config": ...

    clustering: str
    hierarchical: str
    mode: str
    filter_speckle: int
    color_precision: int
    layer_difference: int
    corner_threshold: int
    length_threshold: float
    max_iterations: int
    splice_threshold: int
    simplify: Optional[float]
    path_precision: Optional[int]
    palette: list[str]
    max_colors: Optional[int]
    optimize: int
    binary_threshold: int
    adaptive: bool
    adaptive_window: int
    adaptive_t: float
    watershed_detail: int

    def convert_file(self, input_path: str, output_path: str) -> None: ...
    def convert_bytes(self, data: bytes, format: Optional[str] = None) -> str: ...
    def convert_pixels(self, rgba: bytes, width: int, height: int) -> str: ...

def convert_file(input_path: str, output_path: str, config: Optional[Config] = None) -> None: ...
def convert_bytes(data: bytes, config: Optional[Config] = None, format: Optional[str] = None) -> str: ...
def convert_pixels(rgba: bytes, width: int, height: int, config: Optional[Config] = None) -> str: ...
