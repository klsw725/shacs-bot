use super::*;

#[test]
fn only_a_placeholder_marker_makes_an_image_or_calculation_line() {
    // Given / When / Then
    assert!(is_image_line("[image]"));
    assert!(is_image_line("The chart shows growth [Image]"));
    assert!(is_image_line("[image: chart]"));
    assert!(!is_image_line("[imagery]"));
    assert!(!is_image_line("an image of the room"));
    assert!(!is_image_line("Revenue grew [source: image.md]"));
    assert!(is_calc_line("[calculation]"));
    assert!(is_calc_line("The area is 600 m^2 [Calculation]"));
    assert!(!is_calc_line("[calculations]"));
    assert!(!is_calc_line(
        "The calculation shows 6 [source: calculation.md]"
    ));
}
