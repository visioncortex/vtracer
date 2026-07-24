from typing import Optional

__version__: str

class Config:
    """Conversion configuration. Construct with keyword arguments or a preset,
    mutate via properties, then call one of the ``convert_*`` methods."""

    def __init__(
        self,
        color_mode: str = "color",          # "color" | "bw"
        hierarchical: str = "stacked",      # "stacked" | "cutout" (mosaic)
        mode: str = "spline",               # "pixel" | "polygon" | "spline"
        filter_speckle: int = 4,
        color_precision: int = 6,
        layer_difference: int = 16,
        corner_threshold: int = 60,
        length_threshold: float = 4.0,
        max_iterations: int = 10,
        splice_threshold: int = 45,
        path_precision: int = 2,
        palette: Optional[list[str]] = None,   # e.g. ["#112233", "#445566"]
        max_colors: Optional[int] = None,      # auto-quantize target
        optimize: int = 1,                     # 0 | 1 | 2
    ) -> None: ...

    @staticmethod
    def bw() -> "Config": ...
    @staticmethod
    def poster() -> "Config": ...
    @staticmethod
    def photo() -> "Config": ...

    color_mode: str
    hierarchical: str
    mode: str
    filter_speckle: int
    color_precision: int
    layer_difference: int
    corner_threshold: int
    length_threshold: float
    max_iterations: int
    splice_threshold: int
    path_precision: Optional[int]
    palette: list[str]
    max_colors: Optional[int]
    optimize: int

    def convert_file(self, input_path: str, output_path: str) -> None: ...
    def convert_bytes(self, data: bytes, format: Optional[str] = None) -> str: ...
    def convert_pixels(self, rgba: bytes, width: int, height: int) -> str: ...

def convert_file(input_path: str, output_path: str, config: Optional[Config] = None) -> None: ...
def convert_bytes(data: bytes, config: Optional[Config] = None, format: Optional[str] = None) -> str: ...
def convert_pixels(rgba: bytes, width: int, height: int, config: Optional[Config] = None) -> str: ...
