const Component = ({ children, color }) => (
  <div class="colored">
    {children}
    <style>
      {`
      .colored {
        color: ${color};
      }
      `}
    </style>
  </div>
);

const styles = `
  .red {
    color: red;
  }
`;

export default Component;
export { styles };
