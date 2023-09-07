from typing import Optional

def convert_image_to_svg_py(image_path: str, 
                            out_path: str,   
                            colormode: Optional[str] = None,        # ["color"] or "binary"
                            hierarchical: Optional[str] = None,     # ["stacked"] or "cutout"
                            mode: Optional[str] = None,             # ["spline"], "polygon", "none"
                            filter_speckle: Optional[int] = None,   # default: 4
                            color_precision: Optional[int] = None,  # default: 6
                            layer_difference: Optional[int] = None, # default: 16
                            corner_threshold: Optional[int] = None, # default: 60   
                            length_threshold: Optional[float] = None, # in [3.5, 10] default: 4.0
                            max_iterations: Optional[int] = None,   # default: 10
                            splice_threshold: Optional[int] = None, # default: 45
                            path_precision: Optional[int] = None,   # default: 8
                        ) -> None:
    ...
