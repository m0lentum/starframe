% given an array of angles, prints the PolygonTrigConsts
% used in collider.rs to compute second moments of area
function trig_consts(angles)
  for a_i = 1 : length(angles) - 1
    a_curr = angles(a_i);
    a_next = angles(a_i + 1);

    printf("PolygonTrigConsts {\n");
    printf("\tangle_diff: %.12g,\n", a_next - a_curr);
    printf("\tsin_diff: %.12g,\n", sin(a_next) - sin(a_curr));
    printf("\tsin_double_diff: %.12g,\n", sin(2*a_next) - sin(2*a_curr));
    printf("\tsin_triple_diff: %.12g,\n", sin(3*a_next) - sin(3*a_curr));
    printf("\tcos_diff: %.12g,\n", cos(a_next) - cos(a_curr));
    printf("\tcos_triple_diff: %.12g,\n", cos(3*a_next) - cos(3*a_curr));
    printf("}\n");
  end
end
